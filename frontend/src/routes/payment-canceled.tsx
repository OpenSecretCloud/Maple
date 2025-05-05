import { createFileRoute, Navigate } from "@tanstack/react-router";

export const Route = createFileRoute("/payment-canceled")({
  component: PaymentCanceledPage
});

function PaymentCanceledPage() {
  // Just redirect to pricing page with canceled query parameter
  return <Navigate to="/pricing" search={{ canceled: true }} />;
}
