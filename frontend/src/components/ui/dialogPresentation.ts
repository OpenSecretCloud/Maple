import { cn } from "@/utils/utils";

const DIALOG_CONTENT_BASE_CLASS =
  "fixed left-[50%] top-[50%] z-50 grid w-full max-w-lg translate-x-[-50%] translate-y-[-50%] gap-4 bg-background p-6 shadow-lg duration-200 data-[state=open]:animate-in data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=open]:fade-in-0 data-[state=closed]:zoom-out-95 data-[state=open]:zoom-in-95 dark:bg-muted sm:rounded-2xl";

const DIALOG_CONTENT_CENTERED_SLIDE_CLASS =
  "data-[state=closed]:slide-out-to-left-1/2 data-[state=closed]:slide-out-to-top-[48%] data-[state=open]:slide-in-from-left-1/2 data-[state=open]:slide-in-from-top-[48%]";

export function dialogViewportFrameClassName(bounded: boolean) {
  return cn(
    "contents",
    bounded &&
      "pointer-events-none fixed inset-x-0 top-[var(--maple-dialog-viewport-center-y)] z-50 flex h-[var(--maple-dialog-viewport-available-height)] translate-y-[-50%] flex-col items-center justify-center overflow-hidden"
  );
}

export function dialogContentClassName(bounded: boolean, className?: string) {
  return cn(
    DIALOG_CONTENT_BASE_CLASS,
    !bounded && DIALOG_CONTENT_CENTERED_SLIDE_CLASS,
    bounded &&
      "pointer-events-auto relative left-auto top-auto min-h-0 max-h-full shrink translate-x-0 translate-y-0 overflow-y-auto overscroll-y-contain",
    className
  );
}
