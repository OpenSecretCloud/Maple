import { createFileRoute, Navigate } from "@tanstack/react-router";
import { AccountSettings } from "@/components/settings/AccountSettings";
import { useCompactSettingsLayout } from "@/components/settings/useCompactSettingsLayout";

export const Route = createFileRoute("/settings/")({
  component: SettingsIndex
});

function SettingsIndex() {
  const isCompactSettingsLayout = useCompactSettingsLayout();

  if (!isCompactSettingsLayout) {
    return <Navigate to="/settings/account" replace />;
  }

  return <AccountSettings />;
}
