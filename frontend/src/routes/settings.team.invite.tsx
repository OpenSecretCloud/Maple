import { createFileRoute } from "@tanstack/react-router";
import { TeamInviteSettings } from "@/components/settings/team/TeamInviteSettings";

export const Route = createFileRoute("/settings/team/invite")({
  component: TeamInviteSettings
});
