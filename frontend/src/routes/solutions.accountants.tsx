import { createFileRoute } from "@tanstack/react-router";
import { VerticalLandingMock } from "@/components/VerticalLandingMock";

export const Route = createFileRoute("/solutions/accountants")({
  component: Page
});

function Page() {
  return (
    <VerticalLandingMock
      title="AI for Accountants"
      subtitle="Vertical landing — client financial data stays encrypted end-to-end. (Mock.)"
      bullets={[
        "Tax season workflows and document-heavy tasks",
        "Trust and encryption story tuned for CPA firms",
        "Download + team pricing paths"
      ]}
    />
  );
}
