import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { useOpenSecret } from "@opensecret/react";
import { AlertDestructive } from "@/components/AlertDestructive";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Loader2 } from "lucide-react";

export const Route = createFileRoute("/verify/$code")({
  component: VerifyEmail
});

function VerifyEmail() {
  const { code } = Route.useParams();
  const [isVerifying, setIsVerifying] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const navigate = useNavigate();
  const { verifyEmail, refetchUser } = useOpenSecret();

  useEffect(() => {
    async function verify() {
      try {
        await verifyEmail(code);

        // Do both refetch and navigation after a delay
        setTimeout(async () => {
          await refetchUser();
          navigate({ to: "/" });
        }, 2000);
      } catch (err) {
        if (err instanceof Error) {
          setError(err.message);
        } else {
          setError("An unknown error occurred");
        }
      } finally {
        setIsVerifying(false);
      }
    }
    verify();
  }, [code, navigate, verifyEmail, refetchUser]);

  if (isVerifying) {
    return (
      <Card className="max-w-md mx-auto mt-20">
        <CardHeader>
          <CardTitle>Verifying Email</CardTitle>
        </CardHeader>
        <CardContent className="flex justify-center">
          <Loader2 className="h-8 w-8 animate-spin" />
        </CardContent>
      </Card>
    );
  }

  if (error) {
    return (
      <Card className="max-w-md mx-auto mt-20">
        <CardHeader>
          <CardTitle>Verification Failed</CardTitle>
        </CardHeader>
        <CardContent>
          <AlertDestructive title="Error" description={error} />
        </CardContent>
      </Card>
    );
  }

  return (
    <Card className="max-w-md mx-auto mt-20">
      <CardHeader>
        <CardTitle>Email Verified</CardTitle>
      </CardHeader>
      <CardContent>Your email has been successfully verified. Redirecting...</CardContent>
    </Card>
  );
}
