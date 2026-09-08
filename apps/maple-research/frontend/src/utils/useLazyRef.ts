import { useRef, useState, type MutableRefObject } from "react";

/**
 * Creates a ref value once per component mount.
 *
 * Use this instead of passing a newly allocated object directly to `useRef`,
 * because React evaluates the `useRef` argument again on every render.
 */
export function useLazyRef<T extends object | symbol>(factory: () => T): MutableRefObject<T> {
  const [initialValue] = useState(factory);
  return useRef(initialValue);
}
