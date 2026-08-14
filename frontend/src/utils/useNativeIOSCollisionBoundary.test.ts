import { describe, expect, test } from "bun:test";
import {
  NATIVE_IOS_COMPACT_VIEWPORT_CLASS,
  NATIVE_IOS_SAFE_AREA_BOUNDARY_ID
} from "./nativeIOSViewport";
import { nativeIOSCollisionBoundary } from "./useNativeIOSCollisionBoundary";

function collisionDocument({ native }: { native: boolean }) {
  const boundary = {} as Element;

  return {
    boundary,
    document: {
      documentElement: {
        classList: {
          contains: (token: string) => token === NATIVE_IOS_COMPACT_VIEWPORT_CLASS && native
        }
      },
      getElementById: (id: string) => (id === NATIVE_IOS_SAFE_AREA_BOUNDARY_ID ? boundary : null)
    }
  };
}

describe("native iOS collision boundary", () => {
  test("uses the safe-area element only inside the native compact viewport", () => {
    const native = collisionDocument({ native: true });
    const web = collisionDocument({ native: false });

    expect(nativeIOSCollisionBoundary(native.document)).toBe(native.boundary);
    expect(nativeIOSCollisionBoundary(web.document)).toBeNull();
    expect(nativeIOSCollisionBoundary(undefined)).toBeNull();
  });
});
