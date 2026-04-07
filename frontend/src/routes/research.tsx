import { createFileRoute } from "@tanstack/react-router";
import { VerticalLandingMock } from "@/components/VerticalLandingMock";

export const Route = createFileRoute("/research")({
  component: ResearchPage
});

function ResearchPage() {
  return (
    <VerticalLandingMock
      title="Maple Research"
      subtitle="Hero product page — long-context research and analysis in a verifiable environment. (Copy and visuals TBD.)"
      bullets={[
        "Explain Research vs Agent and when to use each",
        "Login may route into the Research app experience (per product spec)",
        "Download and pricing links from this page"
      ]}
    />
  );
}
