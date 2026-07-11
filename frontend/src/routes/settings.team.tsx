import { createFileRoute, Outlet } from "@tanstack/react-router";

export const Route = createFileRoute("/settings/team")({
  component: TeamSettingsRoute
});

function TeamSettingsRoute() {
  return <Outlet />;
}
