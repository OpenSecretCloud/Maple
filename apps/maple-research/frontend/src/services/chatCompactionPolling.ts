export type ChatCompactionPollScheduler = {
  schedule: (callback: () => void, delayMs: number) => unknown;
  cancel: (handle: unknown) => void;
};

const scheduler: ChatCompactionPollScheduler = {
  schedule: (callback, delayMs) => setTimeout(callback, delayMs),
  cancel: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>)
};

/**
 * Progress is advisory. Never retries the inference POST or controls its lifetime.
 * At most one read is pending; abort and stale-run fences also reject late results.
 */
export function startChatCompactionPolling<T>(options: {
  read: (signal: AbortSignal) => Promise<T>;
  onStatus: (status: T) => void;
  onUnavailable?: () => void;
  isCurrent: () => boolean;
  signal: AbortSignal;
  isUnsupported: (error: unknown) => boolean;
  scheduler?: ChatCompactionPollScheduler;
}): () => void {
  const clock = options.scheduler ?? scheduler;
  const controller = new AbortController();
  let stopped = false;
  let timer: unknown;
  let failures = 0;

  const stop = () => {
    if (stopped) return;
    stopped = true;
    if (timer !== undefined) clock.cancel(timer);
    controller.abort();
    options.signal.removeEventListener("abort", stop);
  };

  const poll = async () => {
    timer = undefined;
    if (stopped || !options.isCurrent()) {
      stop();
      return;
    }
    try {
      const status = await options.read(controller.signal);
      if (stopped || !options.isCurrent()) {
        stop();
        return;
      }
      failures = 0;
      options.onStatus(status);
    } catch (error) {
      // No raw errors: a transport/provider error can contain private material.
      if (stopped || !options.isCurrent()) {
        stop();
        return;
      }
      // Losing visibility is not evidence that compaction completed. Clear the
      // advisory indicator until a later read observes active work again.
      options.onUnavailable?.();
      if (options.isUnsupported(error)) {
        stop();
        return;
      }
      failures += 1;
    }
    if (!stopped && options.isCurrent()) {
      timer = clock.schedule(() => void poll(), Math.min(1000 * 2 ** failures, 10000));
    } else {
      stop();
    }
  };

  if (options.signal.aborted) return stop;
  options.signal.addEventListener("abort", stop, { once: true });
  // Give the POST a chance to register the execution before the first read.
  timer = clock.schedule(() => void poll(), 500);
  return stop;
}
