import { describe, expect, mock, test } from "bun:test";
import { createUpdateService, type PreparedUpdate, type UpdaterPreferences } from "./updateService";

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

  test("uses native prepared-update state and exact-version install commands", async () => {
    const prepared: PreparedUpdate = {
      status: "ready_to_install",
      version: "4.5.6",
      requires_system_approval: false
    };
    const ready: PreparedUpdate = { status: "ready_to_restart", version: "4.5.6" };
    const invoke = mock(async (command: string, args?: Record<string, unknown>) => {
      if (command === "get_prepared_update") return prepared;
      if (command === "install_pending_update") {
        expect(args).toEqual({ expectedVersion: "4.5.6" });
        return ready;
      }
      if (command === "restart_for_update") return undefined;
      throw new Error(`Unexpected command: ${command}`);
    });
    const service = createUpdateService(invoke);

    await expect(service.getPreparedUpdate()).resolves.toEqual(prepared);
    await expect(service.installPreparedUpdate("4.5.6")).resolves.toEqual(ready);
    await expect(service.restartForUpdate()).resolves.toBeUndefined();
  });

  test("subscribes to both native prepared-update transitions and cleans them up", async () => {
    const handlers = new Map<string, (event: { payload: never }) => void>();
    const unlistenAvailable = mock(() => {});
    const unlistenReady = mock(() => {});
    const listen = mock(async (event: string, handler: (event: { payload: never }) => void) => {
      handlers.set(event, handler);
      return event === "update-available" ? unlistenAvailable : unlistenReady;
    });
    const service = createUpdateService(
      mock(async () => undefined),
      listen as Parameters<typeof createUpdateService>[1]
    );
    const received: PreparedUpdate[] = [];

    const unsubscribe = await service.subscribePreparedUpdates((update) => received.push(update));
    handlers.get("update-available")?.({
      payload: {
        version: "4.5.6",
        requires_system_approval: true
      } as never
    });
    handlers.get("update-ready")?.({ payload: { version: "4.5.6" } as never });

    expect(received).toEqual([
      {
        status: "ready_to_install",
        version: "4.5.6",
        requires_system_approval: true
      },
      { status: "ready_to_restart", version: "4.5.6" }
    ]);

    unsubscribe();
    expect(unlistenAvailable).toHaveBeenCalledTimes(1);
    expect(unlistenReady).toHaveBeenCalledTimes(1);
  });
});
