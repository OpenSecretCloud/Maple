import { useEffect, useState } from "react";
import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import { ApiCreditsSettings } from "@/components/settings/api/ApiCreditsSettings";

type ApiCreditsSearch = {
  credits_success?: boolean;
};

export const Route = createFileRoute("/settings/api/")({
  component: ApiCreditsRoute,
  validateSearch: (search: Record<string, unknown>): ApiCreditsSearch => ({
    credits_success:
      search.credits_success === true || search.credits_success === "true" ? true : undefined
  })
});

function ApiCreditsRoute() {
  const { credits_success } = Route.useSearch();
  const [showCreditSuccessMessage] = useState(credits_success === true);
  const queryClient = useQueryClient();
  const navigate = useNavigate();

  useEffect(() => {
    if (!credits_success) return;

    void queryClient.invalidateQueries({ queryKey: ["apiCreditBalance"] });
    void navigate({ to: "/settings/api", search: {}, replace: true });
  }, [credits_success, navigate, queryClient]);

  return <ApiCreditsSettings showCreditSuccessMessage={showCreditSuccessMessage} />;
}
