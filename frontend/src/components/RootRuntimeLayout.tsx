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
 * Keeps account-scoped chat state around the persistent authenticated home only.
 * The OAuth callback route alone survives the signed-out-to-user transition so its
 * one-shot effect cannot replay. Every other route and global account-scoped UI
 * retains the previous account-keyed remount behavior.
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

  return (
    <>
      <ChatRuntimeProvider key={`chat:${accountScopeKey}`}>{authenticatedHome}</ChatRuntimeProvider>
      <Fragment key={routeScopeKey}>{routeContent}</Fragment>
      <Fragment key={`global:${accountScopeKey}`}>{accountScopedUi}</Fragment>
    </>
  );
}
