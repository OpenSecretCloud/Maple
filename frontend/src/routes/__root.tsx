import { useOpenSecret } from "@opensecret/react";
import { OpenSecretContextType } from "@opensecret/react";
import { createRootRouteWithContext, Outlet } from "@tanstack/react-router";

interface RootRouterContext {
  os: OpenSecretContextType;
}

export const Route = createRootRouteWithContext<RootRouterContext>()({
  component: Root
});

function Root() {
  const { auth } = useOpenSecret();

  // TODO... put something here, but showing nothing looks nicer than "Loading..."
  if (auth.loading) {
    return <></>;
  }

  return <Outlet />;
}
