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
import { CheckCircle, XCircle, Trash, Sun, Moon, Monitor } from "lucide-react";
import { ChangePasswordDialog } from "./ChangePasswordDialog";
import { useLocalState } from "@/state/useLocalState";
import { DeleteAccountDialog } from "./DeleteAccountDialog";
import { PreferencesDialog } from "./PreferencesDialog";
import { useTheme } from "@/contexts/ThemeContext";

export function AccountDialog() {
  const os = useOpenSecret();
  const [isChangePasswordOpen, setIsChangePasswordOpen] = useState(false);
  const [isDeleteAccountOpen, setIsDeleteAccountOpen] = useState(false);
  const [isPreferencesOpen, setIsPreferencesOpen] = useState(false);
  const [verificationStatus, setVerificationStatus] = useState<"unverified" | "pending">(
    "unverified"
  );
  const { billingStatus } = useLocalState();
  const { theme, setTheme } = useTheme();

  // Check user login method
  const isEmailUser = os.auth.user?.user.login_method === "email";
  const isGuestUser = os.auth.user?.user.login_method?.toLowerCase() === "guest";

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
          {!isGuestUser && (
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
                  <CheckCircle className="text-maple-success" />
                ) : (
                  <XCircle className="text-maple-error" />
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
          )}
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
                  <SelectItem value={billingStatus?.product_name || "loading"}>
                    {billingStatus ? `${billingStatus.product_name} Plan` : "Loading..."}
                  </SelectItem>
                </SelectGroup>
              </SelectContent>
            </Select>
            {billingStatus?.current_period_end && (
              <div className="text-sm text-muted-foreground">
                {billingStatus.payment_provider === "subscription_pass" ||
                billingStatus.payment_provider === "zaprite"
                  ? "Expires on "
                  : "Renews on "}
                {new Date(Number(billingStatus.current_period_end) * 1000).toLocaleDateString(
                  undefined,
                  {
                    year: "numeric",
                    month: "long",
                    day: "numeric"
                  }
                )}
              </div>
            )}
          </div>
          <div className="grid gap-2">
            <Label>Theme</Label>
            <div className="flex gap-2">
              <Button
                type="button"
                variant={theme === "light" ? "primary" : "outline"}
                size="sm"
                className="flex-1"
                onClick={(e) => {
                  e.preventDefault();
                  setTheme("light");
                }}
              >
                <Sun className="mr-2 h-4 w-4" />
                Light
              </Button>
              <Button
                type="button"
                variant={theme === "dark" ? "primary" : "outline"}
                size="sm"
                className="flex-1"
                onClick={(e) => {
                  e.preventDefault();
                  setTheme("dark");
                }}
              >
                <Moon className="mr-2 h-4 w-4" />
                Dark
              </Button>
              <Button
                type="button"
                variant={theme === "system" ? "primary" : "outline"}
                size="sm"
                className="flex-1"
                onClick={(e) => {
                  e.preventDefault();
                  setTheme("system");
                }}
              >
                <Monitor className="mr-2 h-4 w-4" />
                System
              </Button>
            </div>
          </div>
          <div className="flex flex-col space-y-2">
            <Button
              variant="outline"
              onClick={(e) => {
                e.preventDefault();
                setIsPreferencesOpen(true);
              }}
              type="button"
            >
              User Preferences
            </Button>
            {(isEmailUser || isGuestUser) && (
              <DialogTrigger asChild>
                <Button
                  onClick={(e) => {
                    e.preventDefault(); // Prevent form submission
                    setIsChangePasswordOpen(true);
                  }}
                  type="button" // Explicitly set type to button
                >
                  Change Password
                </Button>
              </DialogTrigger>
            )}
            <Button
              variant="ghost"
              className="text-destructive hover:bg-destructive/10 hover:text-destructive dark:hover:bg-destructive/10"
              onClick={(e) => {
                e.preventDefault(); // Prevent form submission
                setIsDeleteAccountOpen(true);
              }}
              type="button" // Explicitly set type to button to prevent form submission
            >
              <Trash className="mr-2 h-4 w-4" />
              Delete Account
            </Button>
          </div>
        </form>
        <DialogFooter>
          <Button type="submit" disabled>
            Save Changes
          </Button>
        </DialogFooter>
      </DialogContent>
      {(isEmailUser || isGuestUser) && (
        <ChangePasswordDialog open={isChangePasswordOpen} onOpenChange={setIsChangePasswordOpen} />
      )}
      <DeleteAccountDialog open={isDeleteAccountOpen} onOpenChange={setIsDeleteAccountOpen} />
      <PreferencesDialog open={isPreferencesOpen} onOpenChange={setIsPreferencesOpen} />
    </>
  );
}
