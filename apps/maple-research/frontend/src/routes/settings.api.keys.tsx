import { createFileRoute, Outlet } from "@tanstack/react-router";

export const Route = createFileRoute("/settings/api/keys")({
  component: ApiKeysLayout
});

function ApiKeysLayout() {
  return <Outlet />;
}
