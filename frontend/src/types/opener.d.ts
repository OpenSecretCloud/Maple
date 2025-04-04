declare module "@tauri-apps/plugin-opener" {
  /**
   * Open a path with the default app or with a specified app.
   *
   * @param path The file path or URL to open.
   * @param app The app name to open the file with.
   * @returns A promise that resolves when the file is opened.
   */
  export function openPath(path: string, app?: string): Promise<void>;
}
