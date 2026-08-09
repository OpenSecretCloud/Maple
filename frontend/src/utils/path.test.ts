import { describe, expect, it } from "bun:test";
import { truncatePathMiddle } from "./path";

describe("truncatePathMiddle", () => {
  it("leaves a short path unchanged", () => {
    const path = "/Users/admin/Projects/Maple";

    expect(truncatePathMiddle(path)).toBe(path);
  });

  it("keeps the first and last two segments of a long POSIX path", () => {
    expect(
      truncatePathMiddle(
        "/Users/admin/Documents/OpenSecretCloud/Maple/worktrees/agent-mode-sidebar/maple"
      )
    ).toBe("/Users/admin/…/agent-mode-sidebar/maple");
  });

  it("keeps the drive and the first and last two segments of a Windows path", () => {
    expect(
      truncatePathMiddle(
        "C:\\Users\\Admin\\Documents\\OpenSecretCloud\\Maple\\worktrees\\Maple\\Sidebar"
      )
    ).toBe("C:\\Users\\Admin\\…\\Maple\\Sidebar");
  });

  it("keeps the server, share, and final segments of a UNC path", () => {
    expect(
      truncatePathMiddle(
        "\\\\fileserver\\engineering\\teams\\desktop\\OpenSecretCloud\\worktrees\\Maple\\sidebar"
      )
    ).toBe("\\\\fileserver\\engineering\\…\\Maple\\sidebar");
  });

  it("preserves a trailing separator", () => {
    expect(
      truncatePathMiddle(
        "/Users/admin/Documents/OpenSecretCloud/Maple/worktrees/agent-mode-sidebar/maple/"
      )
    ).toBe("/Users/admin/…/agent-mode-sidebar/maple/");
  });

  it.each(["/", "C:\\", "\\\\fileserver\\engineering", "relative-project"])(
    "leaves the root or short path %s unchanged",
    (path) => {
      expect(truncatePathMiddle(path)).toBe(path);
    }
  );

  it("uses the provided threshold while retaining whole path segments", () => {
    expect(truncatePathMiddle("one/two/three/four/five", 20)).toBe("one/two/…/four/five");
  });

  it("reduces retained leading segments before slicing through folder names", () => {
    expect(
      truncatePathMiddle(
        "/Users/an-unusually-long-account-name/Documents/OpenSecretCloud/Maple/worktrees/sidebar",
        40
      )
    ).toBe("/Users/…/worktrees/sidebar");
  });

  it("preserves whole segments in a common four-segment path", () => {
    expect(truncatePathMiddle("/Users/an-unusually-long-account-name/Projects/Maple", 32)).toBe(
      "/Users/…/Projects/Maple"
    );
  });

  it("bounds a path with an unusually long component while retaining both ends", () => {
    const path =
      "/Users/an-unusually-long-account-name/Projects/an-unusually-long-maple-project-name";
    const result = truncatePathMiddle(path, 20);

    expect(Array.from(result)).toHaveLength(20);
    expect(result.startsWith("/Users/an-")).toBe(true);
    expect(result.endsWith("ject-name")).toBe(true);
    expect(result.match(/…/g)).toHaveLength(1);
  });
});
