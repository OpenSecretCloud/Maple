import { Link, createFileRoute } from "@tanstack/react-router";
import { useOpenSecret } from "@opensecret/react";
import { AppEntryPage } from "@/components/AppEntryPage";
import { useRouteMeta } from "@/utils/routeMeta";
import { appUrl } from "@/config/domains";
import { isTauri, isTauriDesktop, isTauriMobile } from "@/utils/platform";
import { AgentMode } from "@/components/AgentMode";
import { RemoteAgentReadOnlyMode } from "@/components/RemoteAgentReadOnlyMode";
import { MapleWordmark } from "@/components/MapleWordmark";
import { Button } from "@/components/ui/button";
import { useAgentPortableRuntime } from "@/contexts/AgentPortableRuntimeContext";
import {
  agentRouteProjectionKey,
  agentRemoteReadOnlyProjectionKey,
  resolveAgentRouteRuntime,
  type AgentRouteRuntimeState,
  type AgentRouteUnavailableReason
} from "@/services/agentRouteRuntime";

export const Route = createFileRoute("/agent")({
  component: AgentRoute
});

function AgentRoute() {
  const os = useOpenSecret();
  const portableRuntime = useAgentPortableRuntime();
  const userId = os.auth.user?.user.id;
  const runtime = userId
    ? resolveAgentRouteRuntime({
        accountId: userId,
        platform: {
          isTauri: isTauri(),
          isTauriDesktop: isTauriDesktop(),
          isTauriMobile: isTauriMobile()
        },
        portableRuntime
      })
    : null;

  useRouteMeta({
    title:
      runtime?.status === "ready"
        ? "Maple Agent Mode"
        : runtime?.status === "readOnlyReady"
          ? "Maple Agent History"
          : "Maple AI",
    description: "Maple Agent Mode.",
    canonicalUrl: appUrl("/agent")
  });

  if (!os.auth.user) {
    return <AppEntryPage />;
  }

  if (!runtime) return null;
  return <AgentRouteContent userId={os.auth.user.user.id} runtime={runtime} />;
}

function AgentRouteContent({
  userId,
  runtime
}: {
  userId: string;
  runtime: AgentRouteRuntimeState;
}) {
  if (runtime.status === "ready") {
    return (
      <AgentMode
        key={agentRouteProjectionKey(userId, runtime.service, runtime.runtimeKey)}
        userId={userId}
        agentRuntimeService={runtime.service}
      />
    );
  }

  if (runtime.status === "readOnlyReady") {
    return (
      <RemoteAgentReadOnlyMode
        key={agentRemoteReadOnlyProjectionKey(userId, runtime.client, runtime.runtimeKey)}
        client={runtime.client}
        runtimeKey={runtime.runtimeKey}
      />
    );
  }

  if (runtime.status === "loading") {
    return (
      <AgentRouteState title="Finding your Agent host">
        <span role="status" aria-live="polite">
          Maple is checking the paired hosts available to this account.
        </span>
      </AgentRouteState>
    );
  }

  if (runtime.status === "selectionRequired") {
    return (
      <AgentRouteState title="Choose an Agent host">
        <p>Choose the paired desktop whose persisted Agent history you want to browse.</p>
        <ul className="mt-5 grid gap-2">
          {runtime.targets.map((target) => (
            <li key={target.key}>
              <Button
                type="button"
                variant="outline"
                className="h-auto w-full justify-start whitespace-normal px-4 py-3 text-left"
                onClick={target.select}
              >
                <span>
                  <span className="block font-medium">{target.label}</span>
                  {target.description && (
                    <span className="mt-0.5 block text-xs font-normal text-muted-foreground">
                      {target.description}
                    </span>
                  )}
                </span>
              </Button>
            </li>
          ))}
        </ul>
      </AgentRouteState>
    );
  }

  const copy = unavailableCopy(runtime.reason);
  return (
    <AgentRouteState title={copy.title} showHomeLink>
      {copy.description}
    </AgentRouteState>
  );
}

function AgentRouteState({
  title,
  children,
  showHomeLink = false
}: {
  title: string;
  children: React.ReactNode;
  showHomeLink?: boolean;
}) {
  return (
    <main className="flex min-h-dvh items-center justify-center bg-background p-6 text-center">
      <div className="w-full max-w-sm space-y-4">
        <MapleWordmark className="mx-auto h-4 w-auto" />
        <div className="space-y-2">
          <h1 className="text-lg font-semibold text-foreground">{title}</h1>
          <div className="text-sm text-muted-foreground">{children}</div>
        </div>
        {showHomeLink && (
          <Button asChild variant="outline">
            <Link to="/">Back to chats</Link>
          </Button>
        )}
      </div>
    </main>
  );
}

function unavailableCopy(reason: AgentRouteUnavailableReason): {
  title: string;
  description: string;
} {
  switch (reason) {
    case "requiresTauri":
      return {
        title: "Agent Mode requires the Maple app",
        description:
          "Agent sessions cannot run from this browser. Open Maple on a supported device."
      };
    case "unsupportedTauriClient":
      return {
        title: "Agent Mode isn’t supported here",
        description: "This Maple app does not support local or paired-host Agent sessions."
      };
    case "remoteProviderUnavailable":
      return {
        title: "Remote Agent Mode isn’t ready on this device",
        description:
          "This build cannot verify a paired desktop host, so Maple will not fall back to local execution."
      };
    case "noPairedHost":
      return {
        title: "No paired Agent host",
        description: "Pair this Maple app with a desktop host before browsing its Agent history."
      };
    case "pairingUnavailable":
      return {
        title: "Paired Agent host unavailable",
        description: "Maple could not verify an Agent host for this account."
      };
    case "invalidPortableRuntime":
      return {
        title: "Remote Agent Mode is unavailable",
        description: "Maple refused an invalid execution target instead of running it locally."
      };
  }
}
