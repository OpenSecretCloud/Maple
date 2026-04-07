import { createFileRoute } from "@tanstack/react-router";
import { VerticalLandingMock } from "@/components/VerticalLandingMock";

export const Route = createFileRoute("/agent")({
  component: AgentPage
});

function AgentPage() {
  return (
    <VerticalLandingMock
      title="Maple Agent"
      subtitle="Hero product page — encrypted chat and workflows for professionals. (Copy and visuals TBD.)"
      bullets={[
        "Positioning for daily AI use with privileged data",
        "Download CTAs aligned with the main marketing home",
        "Optional: screenshots, feature grid, vertical-specific proof points"
      ]}
    />
  );
}
