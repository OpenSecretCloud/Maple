import { createFileRoute } from "@tanstack/react-router";
import { PasswordResetRequestForm } from "@/components/PasswordResetRequestForm";

export const Route = createFileRoute("/password-reset")({
  component: PasswordResetRequest
});

function PasswordResetRequest() {
  return (
    <div className="maple-native-ios-static-safe-block mx-auto max-w-md pt-8">
      <PasswordResetRequestForm />
    </div>
  );
}
