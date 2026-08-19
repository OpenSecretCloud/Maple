import { describe, expect, test } from "bun:test";
import { unclassifiedTauriPlatformInfo } from "@/utils/platform";

describe("unclassifiedTauriPlatformInfo", () => {
  test("never guesses Desktop or Mobile when the Tauri OS probe fails", () => {
    expect(unclassifiedTauriPlatformInfo()).toEqual({
      platform: "unknown",
      isTauri: true,
      isIOS: false,
      isAndroid: false,
      isMobile: false,
      isDesktop: false,
      isMacOS: false,
      isWindows: false,
      isLinux: false,
      isWeb: false,
      isTauriDesktop: false,
      isTauriMobile: false
    });
  });
});
