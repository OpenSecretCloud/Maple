import { Outlet, createFileRoute, redirect } from "@tanstack/react-router";

export const Route = createFileRoute("/_auth")({
  beforeLoad: ({ context, location }) => {
    if (context.os.auth.loading) {
      // Still loading, don't redirect yet
      return;
    }
    if (!context.os.auth.user) {
      throw redirect({
        to: "/",
        search: {
          login: "true",
          next: location.pathname
        }
      });
    }
  },
  component: AuthLayout
});

function AuthLayout() {
  return <Outlet />;
}
