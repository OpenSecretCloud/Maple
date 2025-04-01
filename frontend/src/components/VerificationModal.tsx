import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle
} from "@/components/ui/dialog";
import { useOpenSecret } from "@opensecret/react";
import { useState, useEffect } from "react";
import { Loader2, CheckCircle, LogOut } from "lucide-react";
import { Input } from "./ui/input";
import { Label } from "./ui/label";
import { AlertDestructive } from "./AlertDestructive";

export function VerificationModal() {
  const os = useOpenSecret();
  const [isOpen, setIsOpen] = useState(false);
  const [isResending, setIsResending] = useState(false);
  const [justResent, setJustResent] = useState(false);
  const [verificationCode, setVerificationCode] = useState("");
  const [isVerifying, setIsVerifying] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Update open state based on user verification status
  useEffect(() => {
    if (!os.auth.user) {
      setIsOpen(false);
      // Reset form state when modal closes
      setVerificationCode("");
      setError(null);
      setJustResent(false);
    } else {
      setIsOpen(!os.auth.user.user.email_verified);
    }
  }, [os.auth.user]);

  const handleOpenChange = (open: boolean) => {
    // Only allow closing if the email is verified
    if (!open && os.auth.user?.user.email_verified) {
      setIsOpen(false);
      // Reset form state when modal closes
      setVerificationCode("");
      setError(null);
      setJustResent(false);
    }
  };

  const handleResendVerification = async () => {
    try {
      setIsResending(true);
      await os.requestNewVerificationEmail();
      setJustResent(true);
      // Reset the "just resent" state after 30 seconds
      setTimeout(() => setJustResent(false), 30000);
    } catch (error) {
      console.error("Failed to resend verification email:", error);
    } finally {
      setIsResending(false);
    }
  };

  const handleVerifyCode = async () => {
    if (!verificationCode.trim()) return;

    try {
      setError(null);
      setIsVerifying(true);
      await os.verifyEmail(verificationCode);
      await os.refetchUser();
    } catch (err) {
      if (err instanceof Error) {
        setError(err.message);
      } else {
        setError("Failed to verify email. Please check the code and try again.");
      }
    } finally {
      setIsVerifying(false);
    }
  };

  return (
    <Dialog open={isOpen} onOpenChange={handleOpenChange}>
      <DialogContent className="sm:max-w-[425px] [&>button]:hidden">
        <DialogHeader>
          <DialogTitle>Verify Your Email</DialogTitle>
          <DialogDescription>
            Please check your email ({os.auth.user?.user.email}) to verify your account. You'll need
            to verify your email to continue using Maple AI.
          </DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-4 py-4">
          <div className="grid gap-2">
            <Label htmlFor="verification-code">Verification Code</Label>
            <div className="flex gap-2">
              <Input
                id="verification-code"
                value={verificationCode}
                onChange={(e) => setVerificationCode(e.target.value)}
                placeholder="Enter verification code"
              />
              <Button onClick={handleVerifyCode} disabled={isVerifying || !verificationCode.trim()}>
                {isVerifying ? (
                  <>
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                    Verifying...
                  </>
                ) : (
                  "Verify"
                )}
              </Button>
            </div>
            {error && <AlertDestructive title="Verification Failed" description={error} />}
          </div>
          <div className="flex flex-col gap-2">
            {justResent ? (
              <div className="flex items-center gap-2 text-sm text-muted-foreground bg-muted p-3 rounded-md">
                <CheckCircle className="h-4 w-4 text-green-500" />
                Verification email sent! Please check your inbox.
              </div>
            ) : (
              <Button onClick={handleResendVerification} disabled={isResending}>
                {isResending ? (
                  <>
                    <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                    Sending...
                  </>
                ) : (
                  "Resend Verification Email"
                )}
              </Button>
            )}
            <Button variant="outline" onClick={() => os.signOut()} className="gap-2">
              <LogOut className="w-4 h-4" />
              Log Out
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
