import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { AlertDestructive } from "@/components/AlertDestructive";
import { generateSecureSecret, hashSecret, useOpenSecret } from "@opensecret/react";
import { PasswordResetConfirmForm } from "./PasswordResetConfirmForm";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle
} from "@/components/ui/alert-dialog";
import { AlertTriangle } from "lucide-react";
import { isPasswordResetHardeningNoticeEnabled } from "@/utils/passwordResetHardeningFlag";

export function PasswordResetRequestForm() {
  const [email, setEmail] = useState("");
  const [requestedEmail, setRequestedEmail] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showConfirmForm, setShowConfirmForm] = useState(false);
  const [showResetWarning, setShowResetWarning] = useState(false);
  const [secret, setSecret] = useState("");
  const os = useOpenSecret();

  const requestPasswordReset = async () => {
    const nextEmail = email.trim();

    setIsLoading(true);
    setError(null);
    setShowResetWarning(false);
    setRequestedEmail(nextEmail);

    try {
      // TODO: move this logic to the library
      const generatedSecret = generateSecureSecret();
      const hashedSecret = await hashSecret(generatedSecret);
      await os.requestPasswordReset(nextEmail, hashedSecret);
      setSecret(generatedSecret);
      setShowConfirmForm(true);
    } catch (err) {
      setError("Failed to request password reset. Please try again.");
      console.error(err);
    } finally {
      setIsLoading(false);
    }
  };

  const handleSubmit = (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    setError(null);

    if (isPasswordResetHardeningNoticeEnabled()) {
      setShowResetWarning(true);
      return;
    }

    void requestPasswordReset();
  };

  if (showConfirmForm) {
    return <PasswordResetConfirmForm email={requestedEmail} secret={secret} />;
  }

  return (
    <>
      <Card className="bg-card/70 backdrop-blur-sm mx-auto max-w-[45rem]">
        <CardHeader>
          <CardTitle className="text-xl">Reset Password</CardTitle>
          <CardDescription>Enter your email address to request a password reset.</CardDescription>
        </CardHeader>
        <CardContent>
          <form onSubmit={handleSubmit}>
            <div className="grid gap-4">
              <div className="grid gap-2">
                <Label htmlFor="email">Email</Label>
                <Input
                  id="email"
                  type="email"
                  value={email}
                  onChange={(e) => setEmail(e.target.value)}
                  disabled={isLoading}
                  required
                />
              </div>
              {error && <AlertDestructive title="Error" description={error} />}
              <Button type="submit" className="w-full" disabled={isLoading}>
                {isLoading ? "Requesting..." : "Request Password Reset"}
              </Button>
            </div>
          </form>
        </CardContent>
      </Card>
      <AlertDialog open={showResetWarning} onOpenChange={setShowResetWarning}>
        <AlertDialogContent className="max-w-[calc(100vw-2rem)] sm:max-w-lg">
          <AlertDialogHeader>
            <div className="mb-2 flex h-10 w-10 items-center justify-center rounded-full bg-destructive/10 text-destructive">
              <AlertTriangle className="h-5 w-5" aria-hidden="true" />
            </div>
            <AlertDialogTitle>Resetting your password deletes private data</AlertDialogTitle>
            <AlertDialogDescription className="space-y-3">
              <span className="block">
                Password reset is account access recovery. It creates a new private key and removes
                private encrypted content such as chats and saved data.
              </span>
              <span className="block">
                Before continuing, try signing in with your current password or with Apple, Google,
                or GitHub if that is how you created the account.
              </span>
              <span className="block">
                If you can sign in another way, change your password from account settings instead.
              </span>
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={isLoading}>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={() => void requestPasswordReset()} disabled={isLoading}>
              {isLoading ? "Requesting..." : "Continue with Reset"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
