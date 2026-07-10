import { useOpenSecret } from "@opensecret/react";
import { OpenSecretContextType } from "@opensecret/react";
import { createRootRouteWithContext, Outlet } from "@tanstack/react-router";
import { ExternalUrlConfirmHandler } from "@/components/ExternalUrlConfirmHandler";
import { useLayoutEffect } from "react";
import { transitionAgentAuthUser } from "@/services/agentRuntimeService";

interface RootRouterContext {
  os: OpenSecretContextType;
}

export type RootSearchParams = {
  login?: string;
  next?: string;
  selected_plan?: string;
  success?: boolean;
  canceled?: boolean;
  provider?: string;
};

export const Route = createRootRouteWithContext<RootRouterContext>()({
  component: Root,
  validateSearch: (search: Record<string, unknown>): RootSearchParams => ({
    login: typeof search.login === "string" ? search.login : undefined,
    next: typeof search.next === "string" ? search.next : undefined,
    selected_plan: typeof search.selected_plan === "string" ? search.selected_plan : undefined,
    success: typeof search.success === "boolean" ? search.success : undefined,
    canceled: typeof search.canceled === "boolean" ? search.canceled : undefined,
    provider: typeof search.provider === "string" ? search.provider : undefined
  })
});

function Root() {
  const { auth } = useOpenSecret();
  const userId = auth.user?.user.id || null;

  useLayoutEffect(() => {
    // Queue cleanup before route-level passive effects initialize Agent Mode.
    // A failed transition is surfaced by Agent Mode's matching wait gate.
    void transitionAgentAuthUser(userId).catch(() => {});
  }, [userId]);

  // TODO... put something here, but showing nothing looks nicer than "Loading..."
  if (auth.loading) {
    return <></>;
  }

  return (
    <>
      <Outlet />
      <ExternalUrlConfirmHandler />
    </>
  );
}
