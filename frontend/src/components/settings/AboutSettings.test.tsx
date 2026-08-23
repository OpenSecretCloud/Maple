import { afterEach, describe, expect, mock, test } from "bun:test";
import { act, create, type ReactTestInstance, type ReactTestRenderer } from "react-test-renderer";

mock.module("@tanstack/react-router", () => ({
  Link: ({ children, to }: { children: React.ReactNode; to: string }) => <a href={to}>{children}</a>
}));

const { AboutSettings } = await import("./AboutSettings");

function textContent(node: ReactTestInstance): string {
  return node.children
    .map((child) => (typeof child === "string" ? child : textContent(child)))
    .join("");
}

describe("AboutSettings", () => {
  let renderer: ReactTestRenderer | null = null;

  afterEach(() => {
    if (renderer) act(() => renderer?.unmount());
    renderer = null;
  });

  test("shows app downloads in the web view and hides desktop update settings", async () => {
    await act(async () => {
      renderer = create(<AboutSettings supportsDesktopUpdates={false} showAppDownloads />);
      await Promise.resolve();
    });

    expect(textContent(renderer!.root)).toContain("Get the Maple app");
    expect(textContent(renderer!.root)).not.toContain("Automatic updates");
  });

  test("hides both native-only sections on Tauri mobile", () => {
    act(() => {
      renderer = create(<AboutSettings supportsDesktopUpdates={false} showAppDownloads={false} />);
    });

    expect(textContent(renderer!.root)).not.toContain("Automatic updates");
    expect(textContent(renderer!.root)).not.toContain("Get the Maple app");
  });
});
