import { createFileRoute } from "@tanstack/react-router";
import { AboutSettings } from "@/components/settings/AboutSettings";

export const Route = createFileRoute("/settings/about")({
  component: AboutSettings
});
