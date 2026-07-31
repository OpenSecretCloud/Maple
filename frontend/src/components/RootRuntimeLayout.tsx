import { Fragment, type ReactNode } from "react";
import { ChatRuntimeProvider } from "@/contexts/ChatRuntimeContext";

type RootRuntimeLayoutProps = {
  userId: string | null;
  pathname: string;
  authenticatedHome: ReactNode | null;
  routeContent: ReactNode;
  accountScopedUi: ReactNode;
};

const OAUTH_CALLBACK_PATH = /^\/auth\/([^/]+)\/callback\/?$/;

function getAccountScopeKey(userId: string | null): string {
  return userId === null ? "account:signed-out" : `account:user:${userId}`;
}

function getRouteScopeKey(pathname: string, accountScopeKey: string): string {
  const callbackMatch = OAUTH_CALLBACK_PATH.exec(pathname);
  return callbackMatch ? `route:oauth-callback:${callbackMatch[1]}` : `route:${accountScopeKey}`;
}

/**
 * Keeps account-scoped chat state around the persistent authenticated home and
 * ordinary routed content. The OAuth callback route alone stays outside that
 * provider so it survives the signed-out-to-user transition and its one-shot
 * effect cannot replay. Global account-scoped UI retains its previous remount
 * behavior.
 */
export function RootRuntimeLayout({
  userId,
  pathname,
  authenticatedHome,
  routeContent,
  accountScopedUi
}: RootRuntimeLayoutProps) {
  const accountScopeKey = getAccountScopeKey(userId);
  const routeScopeKey = getRouteScopeKey(pathname, accountScopeKey);
  const isOAuthCallback = OAUTH_CALLBACK_PATH.test(pathname);
  const keyedRouteContent = <Fragment key={routeScopeKey}>{routeContent}</Fragment>;

  return (
    <>
      <ChatRuntimeProvider key={`chat:${accountScopeKey}`}>
        {authenticatedHome}
        {!isOAuthCallback ? keyedRouteContent : null}
      </ChatRuntimeProvider>
      {isOAuthCallback ? keyedRouteContent : null}
      <Fragment key={`global:${accountScopeKey}`}>{accountScopedUi}</Fragment>
    </>
  );
}
