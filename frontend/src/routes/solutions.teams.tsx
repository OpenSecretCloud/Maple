import { createFileRoute } from "@tanstack/react-router";
import { VerticalLandingMock } from "@/components/VerticalLandingMock";

export const Route = createFileRoute("/solutions/teams")({
  component: Page
});

function Page() {
  return (
    <VerticalLandingMock
      title="AI for Teams"
      subtitle="Catch-all vertical — collaboration, shared workspaces, and non-industry-specific Maple. (Mock.)"
      bullets={[
        "When you don’t fit a single industry bucket",
        "Team seats, billing, and admin highlights",
        "Link to Maple Agent + Research"
      ]}
    />
  );
}
