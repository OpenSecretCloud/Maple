import { createFileRoute } from "@tanstack/react-router";
import { VerticalLandingMock } from "@/components/VerticalLandingMock";

export const Route = createFileRoute("/solutions/lawyers")({
  component: Page
});

function Page() {
  return (
    <VerticalLandingMock
      title="AI for Lawyers"
      subtitle="Vertical landing — confidentiality, matter isolation, and provable infrastructure. (Mock.)"
      bullets={[
        "Use cases: research, drafting, discovery prep",
        "Compliance framing and links to Proof / attestation",
        "Case studies and firm-ready CTAs"
      ]}
    />
  );
}
