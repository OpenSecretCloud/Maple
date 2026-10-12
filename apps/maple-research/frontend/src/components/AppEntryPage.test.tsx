import { describe, expect, test } from "bun:test";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterContextProvider
} from "@tanstack/react-router";
import { act, create, type ReactTestRenderer } from "react-test-renderer";
import { AppEntryPage } from "./AppEntryPage";

async function authLinks(href: string): Promise<URL[]> {
  const root = createRootRoute();
  const routes = ["/", "/agent", "/login", "/signup"].map((path) =>
    createRoute({ getParentRoute: () => root, path })
  );
  const router = createRouter({
    routeTree: root.addChildren(routes),
    history: createMemoryHistory({ initialEntries: [href] }),
    isServer: false,
    origin: "https://app-dev.maple.ai"
  });
  await router.load();
  let renderer!: ReactTestRenderer;
  await act(async () => {
    renderer = create(
      <RouterContextProvider router={router}>
        <AppEntryPage />
      </RouterContextProvider>
    );
  });
  try {
    return renderer.root
      .findAllByType("a")
      .map((link) => new URL(link.props.href, "https://app-dev.maple.ai"))
      .filter((url) => url.pathname === "/login" || url.pathname === "/signup");
  } finally {
    act(() => renderer.unmount());
  }
}

describe("signed-out app entry", () => {
  for (const destination of [
    "/?conversation_id=conversation-fixture",
    "/?project_id=project-fixture#files",
    "/?credits_success=true",
    "/agent"
  ]) {
    test(`both auth choices preserve ${destination}`, async () => {
      const links = await authLinks(destination);
      expect(links.map((url) => url.pathname).sort()).toEqual(["/login", "/signup"]);
      for (const link of links) {
        // TanStack serializes string search values as JSON when necessary.
        const next = link.searchParams.get("next")!;
        expect(next.startsWith('"') ? JSON.parse(next) : next).toBe(destination);
      }
    });
  }

  test("preserves an explicit safe destination without carrying login controls", async () => {
    const links = await authLinks("/?login=true&next=%2Fsettings%2Fapi%3Ftab%3Dkeys%23balance");
    for (const link of links) {
      expect(link.searchParams.get("next")).toBe("/settings/api?tab=keys#balance");
    }
  });

  test("rejects an external next value for both login and signup", async () => {
    const links = await authLinks("/?next=https%3A%2F%2Fevil.example");
    for (const link of links) expect(link.searchParams.has("next")).toBe(false);
  });
});
