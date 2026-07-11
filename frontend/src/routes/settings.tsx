import { createFileRoute, redirect } from "@tanstack/react-router";
import { SettingsLayout } from "@/components/settings/SettingsLayout";

export const Route = createFileRoute("/settings")({
  beforeLoad: ({ context, location }) => {
    if (!context.os.auth.loading && !context.os.auth.user) {
      throw redirect({
        to: "/login",
        search: { next: location.href },
        replace: true
      });
    }
  },
  component: SettingsLayout
});
