import { useState } from "react";
import { Button } from "@/components/ui/button";
import {
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue
} from "@/components/ui/select";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useOpenSecret } from "@opensecret/react";
import { CheckCircle, XCircle } from "lucide-react";
import { ChangePasswordDialog } from "./ChangePasswordDialog";
import { useLocalState } from "@/state/useLocalState";

export function AccountDialog() {
  const os = useOpenSecret();
  const [isChangePasswordOpen, setIsChangePasswordOpen] = useState(false);
  const [verificationStatus, setVerificationStatus] = useState<"unverified" | "pending">(
    "unverified"
  );
  const { billingStatus } = useLocalState();

  // Check if user is an email user
  const isEmailUser = os.auth.user?.user.login_method === "email";

  const handleResendVerification = async () => {
    try {
      await os.requestNewVerificationEmail();
      setVerificationStatus("pending");
    } catch (error) {
      console.error("Failed to resend verification email:", error);
    }
  };

  return (
    <>
      <DialogContent className="sm:max-w-[425px]">
        <DialogHeader>
          <DialogTitle>Update Your Account</DialogTitle>
          <DialogDescription>Change your email or upgrade your plan.</DialogDescription>
        </DialogHeader>
        <form className="grid gap-4 py-4">
          <div className="grid gap-2">
            <Label htmlFor="email">Email</Label>
            <div className="flex items-center gap-2">
              <Input
                id="email"
                name="email"
                type="email"
                value={os.auth.user?.user.email}
                disabled
              />
              {os.auth.user?.user.email_verified ? (
                <CheckCircle className="dark:text-green-500 text-green-700" />
              ) : (
                <XCircle className="dark:text-red-500 text-red-700" />
              )}
            </div>
            {!os.auth.user?.user.email_verified && (
              <div className="text-sm text-muted-foreground">
                {verificationStatus === "unverified" ? (
                  <>
                    Unverified -{" "}
                    <button
                      type="button"
                      onClick={handleResendVerification}
                      className="text-primary hover:underline focus:outline-none"
                    >
                      Resend verification email
                    </button>
                  </>
                ) : (
                  "Pending - Check your email for verification link"
                )}
              </div>
            )}
          </div>
          <div className="grid gap-2">
            <Label htmlFor="plan">Plan</Label>
            <Select disabled>
              <SelectTrigger>
                <SelectValue
                  placeholder={billingStatus ? `${billingStatus.product_name} Plan` : "Loading..."}
                />
              </SelectTrigger>
              <SelectContent>
                <SelectGroup>
                  <SelectLabel>Plan</SelectLabel>
                  <SelectItem value={billingStatus?.product_name || ""}>
                    {billingStatus ? `${billingStatus.product_name} Plan` : "Loading..."}
                  </SelectItem>
                </SelectGroup>
              </SelectContent>
            </Select>
          </div>
          {isEmailUser && (
            <DialogTrigger asChild>
              <Button onClick={() => setIsChangePasswordOpen(true)}>Change Password</Button>
            </DialogTrigger>
          )}
        </form>
        <DialogFooter>
          <Button type="submit" disabled>
            Save Changes
          </Button>
        </DialogFooter>
      </DialogContent>
      {isEmailUser && (
        <ChangePasswordDialog open={isChangePasswordOpen} onOpenChange={setIsChangePasswordOpen} />
      )}
    </>
  );
}
