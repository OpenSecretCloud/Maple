import { describe, expect, test } from "bun:test";

import {
  getLaunchWorkspacePath,
  getStoredWorkspaceMode,
  rememberWorkspaceMode,
  rememberWorkspaceModeForPath,
  WORKSPACE_MODE_STORAGE_KEY,
  workspaceModeForPath
} from "./workspaceModePreference";

class MemoryStorage {
  private readonly values = new Map<string, string>();

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.values.set(key, value);
  }
}

describe("workspace mode preference", () => {
  test("defaults missing and invalid preferences to Chat Mode", () => {
    const storage = new MemoryStorage();

    expect(getStoredWorkspaceMode(storage)).toBe("chat");

    storage.setItem(WORKSPACE_MODE_STORAGE_KEY, "invalid");
    expect(getStoredWorkspaceMode(storage)).toBe("chat");
  });

  test("persists repeated mode changes for later launches", () => {
    const storage = new MemoryStorage();

    rememberWorkspaceMode("agent", storage);
    expect(getStoredWorkspaceMode(storage)).toBe("agent");

    rememberWorkspaceMode("chat", storage);
    expect(getStoredWorkspaceMode(storage)).toBe("chat");

    rememberWorkspaceMode("agent", storage);
    expect(getStoredWorkspaceMode(storage)).toBe("agent");
  });

  test("ignores unavailable storage", () => {
    const unreadableStorage = {
      getItem: () => {
        throw new Error("unavailable");
      },
      setItem: () => {
        throw new Error("unavailable");
      }
    };

    expect(getStoredWorkspaceMode(unreadableStorage)).toBe("chat");
    expect(() => rememberWorkspaceMode("agent", unreadableStorage)).not.toThrow();
  });

  test("derives a preference only from workspace routes", () => {
    expect(workspaceModeForPath("/")).toBe("chat");
    expect(workspaceModeForPath("/", "?conversation_id=chat-id")).toBe("chat");
    expect(workspaceModeForPath("/agent")).toBe("agent");
    expect(workspaceModeForPath("/settings")).toBeNull();
    expect(workspaceModeForPath("/login")).toBeNull();
  });

  test("does not replace Agent Mode during settings or transient home redirects", () => {
    const storage = new MemoryStorage();

    rememberWorkspaceModeForPath("/agent", "", storage);
    rememberWorkspaceModeForPath("/settings", "", storage);
    rememberWorkspaceModeForPath("/", "?credits_success=true", storage);
    rememberWorkspaceModeForPath("/", "?team_setup=true", storage);
    rememberWorkspaceModeForPath("/", "?api_settings=true", storage);
    rememberWorkspaceModeForPath("/", "?login=true", storage);

    expect(getStoredWorkspaceMode(storage)).toBe("agent");
  });

  test("restores Agent Mode from a bare desktop home launch", () => {
    const storage = new MemoryStorage();
    rememberWorkspaceMode("agent", storage);

    expect(
      getLaunchWorkspacePath(
        {
          pathname: "/",
          search: "",
          hash: "",
          agentModeAvailable: true
        },
        storage
      )
    ).toBe("/agent");
  });

  test("keeps Chat Mode as the default launch mode", () => {
    const storage = new MemoryStorage();

    expect(
      getLaunchWorkspacePath(
        {
          pathname: "/",
          search: "",
          hash: "",
          agentModeAvailable: true
        },
        storage
      )
    ).toBeNull();
  });

  test("does not override unsupported platforms, deep links, or home callbacks", () => {
    const storage = new MemoryStorage();
    rememberWorkspaceMode("agent", storage);

    expect(
      getLaunchWorkspacePath(
        {
          pathname: "/",
          search: "",
          hash: "",
          agentModeAvailable: false
        },
        storage
      )
    ).toBeNull();
    expect(
      getLaunchWorkspacePath(
        {
          pathname: "/settings",
          search: "",
          hash: "",
          agentModeAvailable: true
        },
        storage
      )
    ).toBeNull();
    expect(
      getLaunchWorkspacePath(
        {
          pathname: "/",
          search: "?login=true",
          hash: "",
          agentModeAvailable: true
        },
        storage
      )
    ).toBeNull();
  });
});
