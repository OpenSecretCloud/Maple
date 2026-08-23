import { afterEach, describe, expect, mock, test } from "bun:test";
import { act, create, type ReactTestInstance, type ReactTestRenderer } from "react-test-renderer";

mock.module("@tanstack/react-router", () => ({
  Link: ({ children, to }: { children: React.ReactNode; to: string }) => <a href={to}>{children}</a>
}));

const { AboutSettings, resolveAboutSettingsGating } = await import("./AboutSettings");

function textContent(node: ReactTestInstance): string {
  return node.children
    .map((child) => (typeof child === "string" ? child : textContent(child)))
    .join("");
}

describe("resolveAboutSettingsGating", () => {
  test("shows downloads only in the web view", () => {
    expect(
      resolveAboutSettingsGating({
        isTauriDesktop: () => false,
        isWeb: () => true
      })
    ).toEqual({ supportsDesktopUpdates: false, showAppDownloads: true });
  });

  test("shows desktop updates only on Tauri desktop", () => {
    expect(
      resolveAboutSettingsGating({
        isTauriDesktop: () => true,
        isWeb: () => false
      })
    ).toEqual({ supportsDesktopUpdates: true, showAppDownloads: false });
  });

  test("hides both sections on Tauri mobile", () => {
    expect(
      resolveAboutSettingsGating({
        isTauriDesktop: () => false,
        isWeb: () => false
      })
    ).toEqual({ supportsDesktopUpdates: false, showAppDownloads: false });
  });
});

describe("AboutSettings", () => {
  let renderer: ReactTestRenderer | null = null;

  afterEach(() => {
    if (renderer) act(() => renderer?.unmount());
    renderer = null;
  });

  test("uses the live platform defaults in this web test environment", async () => {
    await act(async () => {
      renderer = create(<AboutSettings />);
      await Promise.resolve();
    });

    expect(textContent(renderer!.root)).toContain("Get the Maple app");
    expect(textContent(renderer!.root)).not.toContain("Automatic updates");
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
