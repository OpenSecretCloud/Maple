import { createFileRoute } from "@tanstack/react-router";
import { SettingsPage } from "@/components/settings/SettingsPage";

type SettingsSearchParams = {
  tab?: string;
  credits_success?: boolean;
};

function validateSearch(search: Record<string, unknown>): SettingsSearchParams {
  return {
    tab: typeof search.tab === "string" ? search.tab : undefined,
    credits_success:
      search?.credits_success === true || search?.credits_success === "true" ? true : undefined
  };
}

export const Route = createFileRoute("/_auth/settings")({
  component: SettingsRoute,
  validateSearch
});

function SettingsRoute() {
  const { tab, credits_success } = Route.useSearch();
  return <SettingsPage initialTab={tab} creditsSuccess={credits_success} />;
}
