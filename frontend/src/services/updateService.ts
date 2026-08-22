import { invoke as tauriInvoke } from "@tauri-apps/api/core";

export interface UpdaterPreferences {
  automatic_updates: boolean;
}

export type UpdateCheckResult =
  | { status: "automatic_updates_disabled" }
  | { status: "up_to_date" }
  | { status: "ready_to_restart"; version: string }
  | { status: "ready_to_install"; version: string }
  | { status: "install_failed"; version: string };

type Invoke = (command: string, args?: Record<string, unknown>) => Promise<unknown>;

export interface UpdateService {
  loadPreferences(): Promise<UpdaterPreferences>;
  savePreferences(preferences: UpdaterPreferences): Promise<void>;
  checkForUpdates(): Promise<UpdateCheckResult>;
}

export function createUpdateService(invoke: Invoke = tauriInvoke): UpdateService {
  return {
    loadPreferences: async () => (await invoke("load_updater_preferences")) as UpdaterPreferences,
    savePreferences: async (preferences) => {
      await invoke("save_updater_preferences", { preferences });
    },
    checkForUpdates: async () => (await invoke("check_for_updates_manually")) as UpdateCheckResult
  };
}

export const updateService = createUpdateService();
