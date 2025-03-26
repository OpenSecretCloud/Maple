import { type ClassValue, clsx } from "clsx";
import { useEffect, useState } from "react";
import { twMerge } from "tailwind-merge";

// Tailwind breakpoint for md: (consistent with Tailwind's default)
export const MD_BREAKPOINT = 768;

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/**
 * Hook to detect if the viewport is mobile size
 * Uses Tailwind's md breakpoint (768px) for consistency
 */
export function useIsMobile() {
  // Initialize with correct value to prevent flash of incorrect content
  // Also handle server-side rendering with typeof window check
  const [isMobile, setIsMobile] = useState(() =>
    typeof window !== "undefined"
      ? window.matchMedia(`(max-width: ${MD_BREAKPOINT - 1}px)`).matches
      : false
  );

  useEffect(() => {
    // Create media query list
    const mediaQuery = window.matchMedia(`(max-width: ${MD_BREAKPOINT - 1}px)`);

    // Function to handle media query changes
    const handleMediaChange = (e: MediaQueryListEvent) => {
      setIsMobile(e.matches);
    };

    // Use addListener for broader browser support
    if ("addEventListener" in mediaQuery) {
      mediaQuery.addEventListener("change", handleMediaChange);
    } else {
      // For older browsers - using type assertion for deprecated method
      (mediaQuery as MediaQueryList).addListener(handleMediaChange);
    }

    // Cleanup
    return () => {
      if ("removeEventListener" in mediaQuery) {
        mediaQuery.removeEventListener("change", handleMediaChange);
      } else {
        // For older browsers - using type assertion for deprecated method
        (mediaQuery as MediaQueryList).removeListener(handleMediaChange);
      }
    };
  }, []);

  return isMobile;
}

export function useClickOutside(
  ref: React.RefObject<HTMLElement>,
  callback: (event: MouseEvent | TouchEvent) => void
) {
  useEffect(() => {
    function handleClickOutside(event: MouseEvent | TouchEvent) {
      if (ref.current && !ref.current.contains(event.target as Node)) {
        callback(event);
      }
    }

    document.addEventListener("mousedown", handleClickOutside);
    document.addEventListener("touchstart", handleClickOutside);

    return () => {
      document.removeEventListener("mousedown", handleClickOutside);
      document.removeEventListener("touchstart", handleClickOutside);
    };
  }, [ref, callback]);
}
