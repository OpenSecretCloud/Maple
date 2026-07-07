import { createFileRoute } from "@tanstack/react-router";
import { useOpenSecret } from "@opensecret/react";
import { AgentMode } from "@/components/AgentMode";
import { AppEntryPage } from "@/components/AppEntryPage";
import { useRouteMeta } from "@/utils/routeMeta";
import { appUrl } from "@/config/domains";

export const Route = createFileRoute("/agent")({
  component: AgentRoute
});

function AgentRoute() {
  const os = useOpenSecret();

  useRouteMeta({
    title: os.auth.user ? "Maple Agent Mode" : "Maple Research | Private AI Workspace",
    description: "Maple Agent Mode.",
    canonicalUrl: appUrl("/agent")
  });

  if (!os.auth.user) {
    return <AppEntryPage />;
  }

  return <AgentMode />;
}
