import { createFileRoute } from "@tanstack/react-router";
import { VerticalLandingMock } from "@/components/VerticalLandingMock";

export const Route = createFileRoute("/solutions/finance")({
  component: Page
});

function Page() {
  return (
    <VerticalLandingMock
      title="AI for Finance"
      subtitle="Vertical landing — analysis and reporting with a security posture fit for regulated data. (Mock.)"
      bullets={[
        "Research, memos, and market commentary",
        "Controls narrative (TEE, attestation, data residency hooks)",
        "Enterprise CTA paths"
      ]}
    />
  );
}
