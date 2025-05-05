import { createFileRoute, Navigate } from "@tanstack/react-router";

export const Route = createFileRoute("/payment-success")({
  component: PaymentSuccessPage
});

function PaymentSuccessPage() {
  // Just redirect to pricing page with success query parameter
  return <Navigate to="/pricing" search={{ success: true }} />;
}
