import { useState, useEffect } from "react";
import { useOpenSecret } from "@opensecret/react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { CheckCircle, XCircle, Trash, KeyRound, Save, Loader2 } from "lucide-react";
import { ChangePasswordDialog } from "@/components/ChangePasswordDialog";
import { DeleteAccountDialog } from "@/components/DeleteAccountDialog";

export function ProfileSection() {
  const os = useOpenSecret();
  const [isChangePasswordOpen, setIsChangePasswordOpen] = useState(false);
  const [isDeleteAccountOpen, setIsDeleteAccountOpen] = useState(false);
  const [verificationStatus, setVerificationStatus] = useState<"unverified" | "pending">(
    "unverified"
  );

  // Preferences state
  const [prompt, setPrompt] = useState("");
  const [instructionId, setInstructionId] = useState<string | null>(null);
  const [isLoadingPrefs, setIsLoadingPrefs] = useState(true);
  const [isSavingPrefs, setIsSavingPrefs] = useState(false);
  const [prefsError, setPrefsError] = useState<string | null>(null);
  const [prefsSuccess, setPrefsSuccess] = useState(false);

  const isEmailUser = os.auth.user?.user.login_method === "email";
  const isGuestUser = os.auth.user?.user.login_method?.toLowerCase() === "guest";

  // Load preferences on mount
  useEffect(() => {
    loadPreferences();
  }, []);

  const loadPreferences = async () => {
    setIsLoadingPrefs(true);
    setPrefsError(null);
    try {
      const response = await os.listInstructions({ limit: 100 });
      const defaultInstruction = response.data.find((inst) => inst.is_default);
      if (defaultInstruction) {
        setInstructionId(defaultInstruction.id);
        setPrompt(defaultInstruction.prompt);
      } else {
        setInstructionId(null);
        setPrompt("");
      }
    } catch (error) {
      console.error("Failed to load preferences:", error);
      setPrefsError("Failed to load preferences. Please try again.");
    } finally {
      setIsLoadingPrefs(false);
    }
  };

  const handleSavePreferences = async (e: React.FormEvent) => {
    e.preventDefault();
    setPrefsError(null);
    setPrefsSuccess(false);
    setIsSavingPrefs(true);

    try {
      if (instructionId) {
        if (prompt.trim() === "") {
          await os.deleteInstruction(instructionId);
          setInstructionId(null);
          setPrompt("");
        } else {
          await os.updateInstruction(instructionId, { prompt });
        }
      } else {
        if (prompt.trim() !== "") {
          const newInstruction = await os.createInstruction({
            name: "User Preferences",
            prompt,
            is_default: true
          });
          setInstructionId(newInstruction.id);
        }
      }
      setPrefsSuccess(true);
    } catch (error) {
      console.error("Failed to save preferences:", error);
      setPrefsError("Failed to save preferences. Please try again.");
    } finally {
      setIsSavingPrefs(false);
    }
  };

  const handleResendVerification = async () => {
    try {
      await os.requestNewVerificationEmail();
      setVerificationStatus("pending");
    } catch (error) {
      console.error("Failed to resend verification email:", error);
    }
  };

  return (
    <div className="space-y-8">
      <div>
        <h2 className="text-2xl font-semibold tracking-tight">Profile</h2>
        <p className="text-muted-foreground mt-1">
          Manage your account information and preferences.
        </p>
      </div>

      {/* Email Section */}
      {!isGuestUser && (
        <div className="space-y-4">
          <h3 className="text-lg font-medium">Email</h3>
          <div className="space-y-2">
            <Label htmlFor="email">Email Address</Label>
            <div className="flex items-center gap-2">
              <Input
                id="email"
                type="email"
                value={os.auth.user?.user.email || ""}
                disabled
                className="max-w-md"
              />
              {os.auth.user?.user.email_verified ? (
                <CheckCircle className="h-5 w-5 dark:text-green-500 text-green-700 flex-shrink-0" />
              ) : (
                <XCircle className="h-5 w-5 dark:text-red-500 text-red-700 flex-shrink-0" />
              )}
            </div>
            {!os.auth.user?.user.email_verified && (
              <p className="text-sm text-muted-foreground">
                {verificationStatus === "unverified" ? (
                  <>
                    Your email is not verified.{" "}
                    <button
                      type="button"
                      onClick={handleResendVerification}
                      className="text-primary hover:underline focus:outline-none"
                    >
                      Resend verification email
                    </button>
                  </>
                ) : (
                  "Verification email sent. Check your inbox."
                )}
              </p>
            )}
          </div>
        </div>
      )}

      {/* Password Section */}
      {(isEmailUser || isGuestUser) && (
        <div className="space-y-4">
          <h3 className="text-lg font-medium">Password</h3>
          <p className="text-sm text-muted-foreground">
            Update your password to keep your account secure.
          </p>
          <Button onClick={() => setIsChangePasswordOpen(true)} className="gap-2">
            <KeyRound className="h-4 w-4" />
            Change Password
          </Button>
        </div>
      )}

      {/* User Preferences (System Prompt) */}
      <div className="space-y-4">
        <h3 className="text-lg font-medium">User Preferences</h3>
        <p className="text-sm text-muted-foreground">
          Customize your default system prompt for AI conversations.
        </p>
        <form onSubmit={handleSavePreferences} className="space-y-3">
          {prefsError && (
            <Alert variant="destructive">
              <AlertDescription>{prefsError}</AlertDescription>
            </Alert>
          )}
          {prefsSuccess && (
            <Alert>
              <AlertDescription>Preferences saved successfully.</AlertDescription>
            </Alert>
          )}
          <Textarea
            value={prompt}
            onChange={(e) => {
              setPrompt(e.target.value);
              setPrefsSuccess(false);
              setPrefsError(null);
            }}
            placeholder="Enter your custom system prompt here..."
            className="min-h-[150px] resize-y"
            disabled={isLoadingPrefs}
          />
          <Button
            type="submit"
            disabled={isLoadingPrefs || isSavingPrefs || prefsSuccess}
            className="gap-2"
          >
            {isLoadingPrefs ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin" />
                Loading...
              </>
            ) : isSavingPrefs ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin" />
                Saving...
              </>
            ) : (
              <>
                <Save className="h-4 w-4" />
                Save Preferences
              </>
            )}
          </Button>
        </form>
      </div>

      {/* Danger Zone */}
      <div className="space-y-4 border-t border-input pt-8">
        <h3 className="text-lg font-medium text-destructive">Danger Zone</h3>
        <p className="text-sm text-muted-foreground">
          Permanently delete your account and all associated data. This action cannot be undone.
        </p>
        <Button
          variant="outline"
          className="border-destructive text-destructive hover:bg-destructive/10 gap-2"
          onClick={() => setIsDeleteAccountOpen(true)}
        >
          <Trash className="h-4 w-4" />
          Delete Account
        </Button>
      </div>

      {/* Dialogs */}
      {(isEmailUser || isGuestUser) && (
        <ChangePasswordDialog open={isChangePasswordOpen} onOpenChange={setIsChangePasswordOpen} />
      )}
      <DeleteAccountDialog open={isDeleteAccountOpen} onOpenChange={setIsDeleteAccountOpen} />
    </div>
  );
}
