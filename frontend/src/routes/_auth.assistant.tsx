import { createFileRoute } from "@tanstack/react-router";
import { AssistantChat } from "@/components/AssistantChat";

export const Route = createFileRoute("/_auth/assistant")({
  component: AssistantChat
});
