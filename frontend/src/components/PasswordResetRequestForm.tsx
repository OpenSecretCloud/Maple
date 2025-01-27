import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { AlertDestructive } from "@/components/AlertDestructive";
import { generateSecureSecret, hashSecret, useOpenSecret } from "@opensecret/react";
import { PasswordResetConfirmForm } from "./PasswordResetConfirmForm";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";

export function PasswordResetRequestForm() {
  const [email, setEmail] = useState("");
  const [isLoading, setIsLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [showConfirmForm, setShowConfirmForm] = useState(false);
  const [secret, setSecret] = useState("");
  const os = useOpenSecret();

  const handleSubmit = async (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    setIsLoading(true);
    setError(null);

    try {
      // TODO: move this logic to the library
      const generatedSecret = generateSecureSecret();
      const hashedSecret = await hashSecret(generatedSecret);
      await os.requestPasswordReset(email, hashedSecret);
      setSecret(generatedSecret);
      setShowConfirmForm(true);
    } catch (err) {
      setError("Failed to request password reset. Please try again.");
      console.error(err);
    } finally {
      setIsLoading(false);
    }
  };

  if (showConfirmForm) {
    return <PasswordResetConfirmForm email={email} secret={secret} />;
  }

  return (
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
  );
}
