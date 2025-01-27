import { createFileRoute } from "@tanstack/react-router";
import { PasswordResetRequestForm } from "@/components/PasswordResetRequestForm";

export const Route = createFileRoute("/password-reset")({
  component: PasswordResetRequest
});

function PasswordResetRequest() {
  return (
    <div className="pt-8 mx-auto max-w-md">
      <PasswordResetRequestForm />
    </div>
  );
}
