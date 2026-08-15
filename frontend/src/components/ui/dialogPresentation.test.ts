import { describe, expect, test } from "bun:test";

import { dialogContentClassName, dialogViewportFrameClassName } from "./dialogPresentation";

function tokens(className: string) {
  return className.split(/\s+/);
}

describe("dialog viewport presentation", () => {
  test("keeps the existing fixed positioning when no native viewport bound is active", () => {
    expect(tokens(dialogViewportFrameClassName(false))).toEqual(["contents"]);
    expect(tokens(dialogContentClassName(false))).toEqual(
      expect.arrayContaining([
        "fixed",
        "left-[50%]",
        "top-[50%]",
        "translate-x-[-50%]",
        "translate-y-[-50%]",
        "data-[state=open]:slide-in-from-left-1/2",
        "data-[state=open]:slide-in-from-top-[48%]"
      ])
    );
  });

  test("bounds the outer frame while preserving a caller's smaller layout cap", () => {
    const frame = tokens(dialogViewportFrameClassName(true));
    const content = tokens(dialogContentClassName(true, "max-h-[80vh] overflow-hidden max-w-4xl"));

    expect(frame).toEqual(
      expect.arrayContaining([
        "h-[var(--maple-dialog-viewport-available-height)]",
        "flex-col",
        "items-center",
        "justify-center",
        "overflow-hidden"
      ])
    );
    expect(content).toEqual(
      expect.arrayContaining([
        "relative",
        "min-h-0",
        "shrink",
        "max-h-[80vh]",
        "overflow-hidden",
        "max-w-4xl"
      ])
    );
    expect(content).not.toContain("h-[var(--maple-dialog-viewport-available-height)]");
    expect(content.some((token) => token.includes("slide-in-from"))).toBe(false);
    expect(content.some((token) => token.includes("slide-out-to"))).toBe(false);
  });

  test("keeps modal scrolling on the bounded Radix content shard", () => {
    const content = tokens(dialogContentClassName(true));

    expect(content).toEqual(
      expect.arrayContaining(["min-h-0", "max-h-full", "overflow-y-auto", "overscroll-y-contain"])
    );
  });
});
