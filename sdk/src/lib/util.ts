import { useEffect, useRef } from "react";

// Implementation that ensures callback only runs once, even in React strict mode
export function useOnMount(callback: () => void) {
  const hasRun = useRef(false);

  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => {
    if (!hasRun.current) {
      hasRun.current = true;
      callback();
    }
  }, []);
}

// Helper function to sleep for a specified duration
export function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
