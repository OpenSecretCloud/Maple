import { createFileRoute } from "@tanstack/react-router";
import { HistorySettings } from "@/components/settings/HistorySettings";

export const Route = createFileRoute("/settings/history")({
  component: HistorySettings
});
