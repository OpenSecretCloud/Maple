import { createFileRoute } from "@tanstack/react-router";
import { VerticalLandingMock } from "@/components/VerticalLandingMock";

export const Route = createFileRoute("/solutions/developers")({
  component: Page
});

function Page() {
  return (
    <VerticalLandingMock
      title="Secure API for Developers"
      subtitle="Maple Proxy API — build on attestable infrastructure. (Mock.)"
      bullets={[
        "API keys, proxy architecture, and code samples",
        "Links to GitHub / open source components",
        "Developer signup or docs entry point"
      ]}
    />
  );
}
