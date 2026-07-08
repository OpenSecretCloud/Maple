import { Navigate, createFileRoute } from "@tanstack/react-router";
import { useOpenSecret } from "@opensecret/react";
import { AppEntryPage } from "@/components/AppEntryPage";
import { useRouteMeta } from "@/utils/routeMeta";
import { appUrl } from "@/config/domains";
import { isTauriDesktop } from "@/utils/platform";
import { AgentMode } from "@/components/AgentMode";

export const Route = createFileRoute("/agent")({
  component: AgentRoute
});

function AgentRoute() {
  const os = useOpenSecret();
  const agentModeAvailable = isTauriDesktop();

  useRouteMeta({
    title: agentModeAvailable && os.auth.user ? "Maple Agent Mode" : "Maple AI",
    description: "Maple Agent Mode.",
    canonicalUrl: appUrl("/agent")
  });

  if (!agentModeAvailable) {
    return <Navigate to="/" replace />;
  }

  if (!os.auth.user) {
    return <AppEntryPage />;
  }

  return <AgentMode />;
}
