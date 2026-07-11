import { createFileRoute, Navigate } from "@tanstack/react-router";

export const Route = createFileRoute("/payment-success-credits")({
  component: PaymentSuccessCreditsPage
});

function PaymentSuccessCreditsPage() {
  return <Navigate to="/" search={{ credits_success: true }} replace />;
}
