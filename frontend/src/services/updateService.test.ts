import { describe, expect, mock, test } from "bun:test";
import { createUpdateService, type UpdaterPreferences } from "./updateService";

describe("updateService", () => {
  test("uses the native app-scoped updater preference commands", async () => {
    const preferences: UpdaterPreferences = { automatic_updates: false };
    const invoke = mock(async (command: string) => {
      if (command === "load_updater_preferences") return preferences;
      if (command === "save_updater_preferences") return undefined;
      throw new Error(`Unexpected command: ${command}`);
    });
    const service = createUpdateService(invoke);

    await expect(service.loadPreferences()).resolves.toEqual(preferences);
    await expect(service.savePreferences(preferences)).resolves.toBeUndefined();
    expect(invoke).toHaveBeenNthCalledWith(1, "load_updater_preferences");
    expect(invoke).toHaveBeenNthCalledWith(2, "save_updater_preferences", { preferences });
  });

  test("runs manual checks independently of the automatic preference", async () => {
    const invoke = mock(async (command: string) => {
      if (command === "check_for_updates_manually") {
        return { status: "up_to_date" };
      }
      throw new Error(`Unexpected command: ${command}`);
    });
    const service = createUpdateService(invoke);

    await expect(service.checkForUpdates()).resolves.toEqual({ status: "up_to_date" });
    expect(invoke).toHaveBeenCalledWith("check_for_updates_manually");
  });
});
