import { describe, expect, it } from "bun:test";
import { getImageFilesFromClipboardItems, maybeReadLinuxTauriClipboardImages } from "./imagePaste";

describe("image paste", () => {
  it("keeps the DOM image path authoritative", () => {
    const image = new File(["png"], "clipboard.png", { type: "image/png" });
    const files = getImageFilesFromClipboardItems([{ type: "image/png", getAsFile: () => image }]);
    let readCalls = 0;

    const fallback = maybeReadLinuxTauriClipboardImages({
      eventItemTypes: ["image/png"],
      isTauri: true,
      isLinux: true,
      readClipboard: () => {
        readCalls++;
        return Promise.resolve([]);
      }
    });

    expect(files).toEqual([image]);
    expect(fallback).toBeUndefined();
    expect(readCalls).toBe(0);
  });

  it("does not inspect the async clipboard for ordinary text paste", () => {
    const files = getImageFilesFromClipboardItems([{ type: "text/plain", getAsFile: () => null }]);
    let readCalls = 0;

    const fallback = maybeReadLinuxTauriClipboardImages({
      eventItemTypes: ["text/plain"],
      isTauri: true,
      isLinux: true,
      readClipboard: () => {
        readCalls++;
        return Promise.resolve([]);
      }
    });

    expect(files).toEqual([]);
    expect(fallback).toBeUndefined();
    expect(readCalls).toBe(0);
  });

  it("starts an async image read immediately for an empty Linux Tauri paste event", async () => {
    let readCalls = 0;
    const fallback = maybeReadLinuxTauriClipboardImages({
      eventItemTypes: [],
      isTauri: true,
      isLinux: true,
      readClipboard: () => {
        readCalls++;
        return Promise.resolve([
          {
            types: ["image/png"],
            getType: () => Promise.resolve(new Blob(["png data"], { type: "image/png" }))
          }
        ]);
      }
    });

    expect(readCalls).toBe(1);
    expect(fallback).toBeDefined();

    const files = await fallback!;
    expect(files).toHaveLength(1);
    expect(files[0].name).toBe("pasted-image.png");
    expect(files[0].type).toBe("image/png");
    expect(await files[0].text()).toBe("png data");
  });

  it("reads an image from an HTML-only browser Copy Image paste", async () => {
    let readCalls = 0;
    const fallback = maybeReadLinuxTauriClipboardImages({
      eventItemTypes: ["text/html"],
      isTauri: true,
      isLinux: true,
      readClipboard: () => {
        readCalls++;
        return Promise.resolve([
          {
            types: ["text/html", "image/png"],
            getType: (type) => Promise.resolve(new Blob([type], { type }))
          }
        ]);
      }
    });

    expect(readCalls).toBe(1);
    expect(fallback).toBeDefined();

    const files = await fallback!;
    expect(files).toHaveLength(1);
    expect(files[0].name).toBe("pasted-image.png");
    expect(files[0].type).toBe("image/png");
  });

  it("does not inspect a paste that includes an ordinary text representation", () => {
    let readCalls = 0;
    const fallback = maybeReadLinuxTauriClipboardImages({
      eventItemTypes: ["text/html", "text/plain"],
      isTauri: true,
      isLinux: true,
      readClipboard: () => {
        readCalls++;
        return Promise.resolve([]);
      }
    });

    expect(fallback).toBeUndefined();
    expect(readCalls).toBe(0);
  });

  it.each([
    ["web", false, false],
    ["non-Tauri Linux", false, true],
    ["Tauri macOS", true, false],
    ["Tauri Windows", true, false],
    ["Tauri iOS", true, false],
    ["Tauri Android", true, false]
  ])("is a no-op on %s", (_platform, isTauri, isLinux) => {
    let readCalls = 0;

    const fallback = maybeReadLinuxTauriClipboardImages({
      eventItemTypes: [],
      isTauri,
      isLinux,
      readClipboard: () => {
        readCalls++;
        return Promise.resolve([]);
      }
    });

    expect(fallback).toBeUndefined();
    expect(readCalls).toBe(0);
  });

  it("uses one supported representation per clipboard item", async () => {
    const requestedTypes: string[] = [];
    const fallback = maybeReadLinuxTauriClipboardImages({
      eventItemTypes: [],
      isTauri: true,
      isLinux: true,
      readClipboard: () =>
        Promise.resolve([
          {
            types: ["image/png", "image/jpeg"],
            getType: (type) => {
              requestedTypes.push(type);
              return Promise.resolve(new Blob(["image"], { type }));
            }
          }
        ])
    });

    const files = await fallback!;
    expect(requestedTypes).toEqual(["image/png"]);
    expect(files).toHaveLength(1);
  });

  it("skips an unreadable item without dropping other clipboard images", async () => {
    const fallback = maybeReadLinuxTauriClipboardImages({
      eventItemTypes: [],
      isTauri: true,
      isLinux: true,
      readClipboard: () =>
        Promise.resolve([
          {
            types: ["image/png"],
            getType: () => Promise.reject(new Error("clipboard item disappeared"))
          },
          {
            types: ["image/webp"],
            getType: () => Promise.resolve(new Blob(["webp"], { type: "image/webp" }))
          }
        ])
    });

    const files = await fallback!;
    expect(files).toHaveLength(1);
    expect(files[0].name).toBe("pasted-image.webp");
    expect(files[0].type).toBe("image/webp");
  });

  it("silently skips unavailable or rejected clipboard reads", async () => {
    const unavailable = maybeReadLinuxTauriClipboardImages({
      eventItemTypes: [],
      isTauri: true,
      isLinux: true
    });
    const rejected = maybeReadLinuxTauriClipboardImages({
      eventItemTypes: [],
      isTauri: true,
      isLinux: true,
      readClipboard: () => Promise.reject(new Error("clipboard access rejected"))
    });

    expect(unavailable).toBeUndefined();
    expect(await rejected!).toEqual([]);
  });
});
