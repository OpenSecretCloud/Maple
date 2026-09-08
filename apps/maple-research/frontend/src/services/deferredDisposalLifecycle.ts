export type DeferredDisposalScheduler = (task: () => void) => void;

const scheduleAfterEffectReplay: DeferredDisposalScheduler = (task) => {
  setTimeout(task, 0);
};

/**
 * Defers ownership cleanup long enough for React Strict Mode to replay an
 * effect. A replayed activation invalidates the first cleanup; a real unmount
 * has no matching activation and therefore disposes the resource once.
 */
export function createDeferredDisposalLifecycle(
  dispose: () => void,
  schedule: DeferredDisposalScheduler = scheduleAfterEffectReplay
) {
  let activation = 0;

  return {
    activate(): () => void {
      const ownedActivation = ++activation;

      return () => {
        schedule(() => {
          if (activation === ownedActivation) {
            dispose();
          }
        });
      };
    }
  };
}
