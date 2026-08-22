import { afterEach, describe, expect, mock, spyOn, test } from "bun:test";
import { act, create, type ReactTestInstance, type ReactTestRenderer } from "react-test-renderer";
import { Switch } from "@/components/ui/switch";
import type { UpdateService } from "@/services/updateService";
import { UpdateSettings } from "./UpdateSettings";

function textContent(node: ReactTestInstance): string {
  return node.children
    .map((child) => (typeof child === "string" ? child : textContent(child)))
    .join("");
}

function service(overrides: Partial<UpdateService> = {}): UpdateService {
  return {
    loadPreferences: mock(async () => ({ automatic_updates: false })),
    savePreferences: mock(async () => {}),
    checkForUpdates: mock(async () => ({ status: "up_to_date" as const })),
    ...overrides
  };
}

describe("UpdateSettings", () => {
  let renderer: ReactTestRenderer | null = null;

  afterEach(() => {
    if (renderer) act(() => renderer?.unmount());
    renderer = null;
  });

  test("loads the app preference and keeps manual checks available when it is off", async () => {
    const updates = service();

    await act(async () => {
      renderer = create(<UpdateSettings service={updates} />);
      await Promise.resolve();
    });

    const toggle = renderer!.root.findByType(Switch);
    expect(toggle.props.checked).toBe(false);
    expect(toggle.props.disabled).toBe(false);
    expect(toggle.props["aria-describedby"]).toBe("automatic-updates-description");

    const checkButton = renderer!.root
      .findAllByType("button")
      .find((button) => textContent(button).includes("Check for updates"));
    expect(checkButton?.props.disabled).toBe(false);
  });

  test("persists a toggle before reflecting the new value", async () => {
    const updates = service();

    await act(async () => {
      renderer = create(<UpdateSettings service={updates} />);
      await Promise.resolve();
    });

    await act(async () => {
      await renderer!.root.findByType(Switch).props.onCheckedChange(true);
    });

    expect(updates.savePreferences).toHaveBeenCalledWith({ automatic_updates: true });
    expect(renderer!.root.findByType(Switch).props.checked).toBe(true);
    expect(
      renderer!.root.findAll((node) => node.props.role === "status").map(textContent)
    ).toContain("Automatic updates are on. Maple will check shortly after launch and every hour.");
  });

  test("keeps the previous value and shows an actionable error when saving fails", async () => {
    const saveError = new Error("disk unavailable");
    const updates = service({
      savePreferences: mock(async () => {
        throw saveError;
      })
    });
    const consoleError = spyOn(console, "error").mockImplementation(() => {});

    await act(async () => {
      renderer = create(<UpdateSettings service={updates} />);
      await Promise.resolve();
    });
    await act(async () => {
      await renderer!.root.findByType(Switch).props.onCheckedChange(true);
    });

    expect(renderer!.root.findByType(Switch).props.checked).toBe(false);
    expect(textContent(renderer!.root.find((node) => node.props.role === "alert"))).toBe(
      "Maple couldn't save your update preference. Please try again."
    );
    consoleError.mockRestore();
  });

  test("does not claim updates are off after a load failure and can explicitly persist off", async () => {
    const updates = service({
      loadPreferences: mock(async () => {
        throw new Error("invalid json");
      })
    });
    const consoleError = spyOn(console, "error").mockImplementation(() => {});

    await act(async () => {
      renderer = create(<UpdateSettings service={updates} />);
      await Promise.resolve();
    });

    const toggle = renderer!.root.findByType(Switch);
    expect(toggle.props.checked).toBe(false);
    expect(toggle.props.disabled).toBe(true);
    expect(textContent(renderer!.root.find((node) => node.props.role === "alert"))).toContain(
      "Automatic update behavior is unchanged"
    );
    const checkButton = renderer!.root
      .findAllByType("button")
      .find((button) => textContent(button).includes("Check for updates"));
    expect(checkButton?.props.disabled).toBe(false);

    await act(async () => {
      checkButton!.props.onClick();
      await Promise.resolve();
    });

    expect(
      renderer!.root
        .findAllByType("button")
        .some((button) => textContent(button).includes("Turn automatic updates off"))
    ).toBe(true);

    const turnOffButton = renderer!.root
      .findAllByType("button")
      .find((button) => textContent(button).includes("Turn automatic updates off"));
    await act(async () => {
      turnOffButton!.props.onClick();
      await Promise.resolve();
    });

    expect(updates.savePreferences).toHaveBeenCalledWith({ automatic_updates: false });
    expect(renderer!.root.findByType(Switch).props.disabled).toBe(false);
    expect(renderer!.root.findByType(Switch).props.checked).toBe(false);
    consoleError.mockRestore();
  });

  test("reports the result of a user-requested check", async () => {
    const updates = service({
      checkForUpdates: mock(async () => ({
        status: "ready_to_restart" as const,
        version: "9.8.7"
      }))
    });

    await act(async () => {
      renderer = create(<UpdateSettings service={updates} />);
      await Promise.resolve();
    });
    const checkButton = renderer!.root
      .findAllByType("button")
      .find((button) => textContent(button).includes("Check for updates"));

    await act(async () => {
      checkButton!.props.onClick();
      await Promise.resolve();
    });

    expect(updates.checkForUpdates).toHaveBeenCalledTimes(1);
    expect(
      renderer!.root.findAll((node) => node.props.role === "status").map(textContent)
    ).toContain("Version 9.8.7 is installed. Restart Maple to finish updating.");
  });

  test("announces a manual install failure as an error", async () => {
    const updates = service({
      checkForUpdates: mock(async () => ({
        status: "install_failed" as const,
        version: "9.8.7"
      }))
    });

    await act(async () => {
      renderer = create(<UpdateSettings service={updates} />);
      await Promise.resolve();
    });
    const checkButton = renderer!.root
      .findAllByType("button")
      .find((button) => textContent(button).includes("Check for updates"));

    await act(async () => {
      checkButton!.props.onClick();
      await Promise.resolve();
    });

    expect(textContent(renderer!.root.find((node) => node.props.role === "alert"))).toContain(
      "Maple couldn't install version 9.8.7"
    );
  });
});
