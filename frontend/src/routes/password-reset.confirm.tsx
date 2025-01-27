import { createFileRoute } from "@tanstack/react-router";
import { PasswordResetConfirmForm } from "@/components/PasswordResetConfirmForm";

export const Route = createFileRoute("/password-reset/confirm")({
  component: PasswordResetConfirm,
  validateSearch: (search: Record<string, unknown>) => {
    return {
      email: search.email as string,
      secret: search.secret as string
    };
  }
});

function PasswordResetConfirm() {
  const { email, secret } = Route.useSearch();

  return (
    <div className="pt-8 mx-auto max-w-md">
      <h1 className="text-2xl font-bold mb-4">Confirm Password Reset</h1>
      <PasswordResetConfirmForm email={email} secret={secret} />
    </div>
  );
}
