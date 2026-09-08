import { afterEach, describe, expect, mock, spyOn, test } from "bun:test";
import { act, create, type ReactTestInstance, type ReactTestRenderer } from "react-test-renderer";
import { GITHUB_RELEASES_LATEST_URL, type DownloadInfo } from "@/utils/githubRelease";
import { DownloadSettings } from "./DownloadSettings";

function textContent(node: ReactTestInstance): string {
  return node.children
    .map((child) => (typeof child === "string" ? child : textContent(child)))
    .join("");
}

const latestInfo: DownloadInfo = {
  version: "9.9.9",
  tagName: "v9.9.9",
  downloadUrls: {
    macOS: "https://example.test/Maple.dmg",
    windowsExe: "https://example.test/Maple.exe",
    linuxAppImage: "https://example.test/Maple.AppImage",
    linuxDeb: "https://example.test/Maple.deb",
    linuxRpm: "https://example.test/Maple.rpm",
    androidApk: "https://example.test/Maple.apk"
  },
  releaseUrl: "https://example.test/releases/v9.9.9"
};

function radioButtons(renderer: ReactTestRenderer): ReactTestInstance[] {
  return renderer.root.findAll((node) => node.props.role === "radio");
}

describe("DownloadSettings", () => {
  let renderer: ReactTestRenderer | null = null;

  afterEach(() => {
    if (renderer) act(() => renderer?.unmount());
    renderer = null;
  });

  test("suggests macOS for a Mac browser and uses the latest dmg URL", async () => {
    const loadDownloadInfo = mock(async () => latestInfo);

    await act(async () => {
      renderer = create(
        <DownloadSettings
          environment={{
            userAgent:
              "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15",
            platform: "MacIntel",
            maxTouchPoints: 0
          }}
          loadDownloadInfo={loadDownloadInfo}
        />
      );
      await Promise.resolve();
    });

    expect(textContent(renderer!.root)).toContain("This browser looks like macOS");
    const macosRadio = radioButtons(renderer!).find((button) =>
      textContent(button).includes("macOS")
    );
    expect(macosRadio?.props["aria-checked"]).toBe(true);
    expect(textContent(macosRadio!)).toContain("Suggested");

    const downloadLink = renderer!.root
      .findAllByType("a")
      .find((link) => textContent(link).includes("Download for macOS"));
    expect(downloadLink?.props.href).toBe("https://example.test/Maple.dmg");
    expect(textContent(renderer!.root)).toContain("9.9.9");
  });

  test("lets the user switch to another platform", async () => {
    const loadDownloadInfo = mock(async () => latestInfo);

    await act(async () => {
      renderer = create(
        <DownloadSettings
          environment={{
            userAgent:
              "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36"
          }}
          loadDownloadInfo={loadDownloadInfo}
        />
      );
      await Promise.resolve();
    });

    const linuxRadio = radioButtons(renderer!).find((button) =>
      textContent(button).includes("Linux")
    );
    await act(async () => {
      linuxRadio?.props.onClick();
    });

    expect(linuxRadio?.props["aria-checked"]).toBe(true);
    expect(linuxRadio?.props.tabIndex).toBe(0);
    expect(
      renderer!.root.findAllByType("a").map((link) => ({
        href: link.props.href,
        label: textContent(link)
      }))
    ).toEqual(
      expect.arrayContaining([
        { href: "https://example.test/Maple.AppImage", label: "Download AppImage" },
        { href: "https://example.test/Maple.deb", label: ".deb" },
        { href: "https://example.test/Maple.rpm", label: ".rpm" }
      ])
    );

    await act(async () => {
      linuxRadio?.props.onKeyDown({
        key: "ArrowDown",
        preventDefault() {}
      });
    });
    const iosRadio = radioButtons(renderer!).find((button) => textContent(button).includes("iOS"));
    expect(iosRadio?.props["aria-checked"]).toBe(true);
  });

  test("keeps latest-release fallback links when the GitHub lookup fails", async () => {
    const loadDownloadInfo = mock(async () => null);
    const consoleError = spyOn(console, "error").mockImplementation(() => {});

    await act(async () => {
      renderer = create(
        <DownloadSettings
          environment={{ userAgent: "UnknownClient/1.0" }}
          loadDownloadInfo={loadDownloadInfo}
        />
      );
      await Promise.resolve();
    });

    const downloadLink = renderer!.root
      .findAllByType("a")
      .find((link) => textContent(link).includes("Download for macOS"));
    expect(downloadLink?.props.href).toBe(GITHUB_RELEASES_LATEST_URL);
    expect(textContent(renderer!.root)).toContain("Choose the platform you want to install.");
    consoleError.mockRestore();
  });
});
