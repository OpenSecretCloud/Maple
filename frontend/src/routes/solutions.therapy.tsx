import { createFileRoute } from "@tanstack/react-router";
import { VerticalLandingMock } from "@/components/VerticalLandingMock";

export const Route = createFileRoute("/solutions/therapy")({
  component: Page
});

function Page() {
  return (
    <VerticalLandingMock
      title="AI for Therapy"
      subtitle="Vertical landing — PHI-sensitive workflows with encryption and verification. (Mock.)"
      bullets={[
        "Session prep, psychoeducation, documentation support (positioning only)",
        "HIPAA-oriented messaging (validate with legal)",
        "Gentle CTAs to Proof and downloads"
      ]}
    />
  );
}
