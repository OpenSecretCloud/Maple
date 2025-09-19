import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { useOpenSecret } from "@opensecret/react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { AlertDestructive } from "@/components/AlertDestructive";
import { Loader2, Github, Mail } from "lucide-react";
import { Google } from "@/components/icons/Google";
import { Apple } from "@/components/icons/Apple";
import { AuthMain } from "@/components/AuthMain";
import { isTauri, invoke } from "@tauri-apps/api/core";
import { v4 as uuidv4 } from "uuid";
import type { AppleCredential } from "@/types/apple-sign-in";
import { sha256 } from "@noble/hashes/sha256";
import { bytesToHex } from "@noble/hashes/utils";
import { AppleAuthProvider } from "@/components/AppleAuthProvider";
import { getBillingService } from "@/billing/billingService";
import { useIsIOS, useIsTauri } from "@/hooks/usePlatform";

type LoginSearchParams = {
  next?: string;
  selected_plan?: string;
};

export const Route = createFileRoute("/login")({
  component: LoginPage,
  validateSearch: (search: Record<string, unknown>): LoginSearchParams => ({
    next: typeof search.next === "string" ? search.next : undefined,
    selected_plan: typeof search.selected_plan === "string" ? search.selected_plan : undefined
  })
});

type LoginMethod = "email" | "github" | "google" | "apple" | null;

function LoginPage() {
  const navigate = useNavigate();
  const os = useOpenSecret();
  const { next, selected_plan } = Route.useSearch();
  const [loginMethod, setLoginMethod] = useState<LoginMethod>(null);
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);

  // Use platform detection hooks
  const { isIOS } = useIsIOS();
  const { isTauri: isTauriEnv } = useIsTauri();

  // Redirect if already logged in
  useEffect(() => {
    if (os.auth.user) {
      if (selected_plan) {
        navigate({
          to: "/pricing",
          search: { selected_plan }
        });
      } else {
        navigate({ to: next || "/" });
      }
    }
  }, [os.auth.user, navigate, next, selected_plan]);

  const handleSubmit = async (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    setIsLoading(true);
    setError(null);
    const formData = new FormData(e.currentTarget);
    const email = formData.get("email") as string;
    const password = formData.get("password") as string;

    try {
      await os.signIn(email, password);
      // Clear any existing billing token to prevent session mixing
      try {
        getBillingService().clearToken();
      } catch (billingError) {
        console.warn("Failed to clear billing token:", billingError);
      }
      setTimeout(() => {
        if (selected_plan) {
          navigate({
            to: "/pricing",
            search: { selected_plan }
          });
        } else {
          navigate({ to: next || "/" });
        }
        window.scrollTo(0, 0);
      }, 100);
    } catch (error) {
      console.error(error);
      if (error instanceof Error) {
        setError(`${error.message}`);
      } else {
        setError("Something went wrong. Please try again.");
      }
    } finally {
      setIsLoading(false);
    }
  };

  const handleGitHubLogin = async () => {
    try {
      const isTauriEnv = await isTauri();
      console.log("[OAuth] Using", isTauriEnv ? "Tauri" : "web", "flow");

      if (isTauriEnv) {
        // For Tauri (desktop or mobile), redirect to the web app's desktop-auth route
        let desktopAuthUrl = "https://trymaple.ai/desktop-auth?provider=github";

        // If there's a selected plan, add it to the URL
        if (selected_plan) {
          desktopAuthUrl += `&selected_plan=${encodeURIComponent(selected_plan)}`;
        }

        // Use the opener plugin by directly invoking the command
        // This works for both desktop and mobile (iOS/Android)
        console.log("[OAuth] Opening URL in external browser:", desktopAuthUrl);
        invoke("plugin:opener|open_url", { url: desktopAuthUrl }).catch((error: Error) => {
          console.error("[OAuth] Failed to open external browser:", error);
          setError("Failed to open authentication page in browser");
        });
      } else {
        // Web flow remains unchanged
        const { auth_url } = await os.initiateGitHubAuth("");
        if (selected_plan) {
          sessionStorage.setItem("selected_plan", selected_plan);
        }
        window.location.href = auth_url;
      }
    } catch (error) {
      console.error("Failed to initiate GitHub login:", error);
      setError("Failed to initiate GitHub login. Please try again.");
    }
  };

  const handleGoogleLogin = async () => {
    try {
      const isTauriEnv = await isTauri();
      console.log("[OAuth] Using", isTauriEnv ? "Tauri" : "web", "flow");

      if (isTauriEnv) {
        // For Tauri (desktop or mobile), redirect to the web app's desktop-auth route
        let desktopAuthUrl = "https://trymaple.ai/desktop-auth?provider=google";

        // If there's a selected plan, add it to the URL
        if (selected_plan) {
          desktopAuthUrl += `&selected_plan=${encodeURIComponent(selected_plan)}`;
        }

        // Use the opener plugin by directly invoking the command
        // This works for both desktop and mobile (iOS/Android)
        console.log("[OAuth] Opening URL in external browser:", desktopAuthUrl);
        invoke("plugin:opener|open_url", { url: desktopAuthUrl }).catch((error: Error) => {
          console.error("[OAuth] Failed to open external browser:", error);
          setError("Failed to open authentication page in browser");
        });
      } else {
        // Web flow remains unchanged
        const { auth_url } = await os.initiateGoogleAuth("");
        if (selected_plan) {
          sessionStorage.setItem("selected_plan", selected_plan);
        }
        window.location.href = auth_url;
      }
    } catch (error) {
      console.error("Failed to initiate Google login:", error);
      setError("Failed to initiate Google login. Please try again.");
    }
  };

  const handleAppleLogin = async () => {
    try {
      if (isTauriEnv && isIOS) {
        // Native iOS implementation using Apple Sign In plugin
        console.log("[OAuth] Initiating native Sign in with Apple for iOS");

        try {
          // Generate random UUIDs for state and nonce
          const state = uuidv4();
          const rawNonce = uuidv4();

          // SHA-256 hash the nonce before sending to Apple
          // Apple requires the nonce to be hashed with SHA-256
          const hashedNonce = bytesToHex(sha256(new TextEncoder().encode(rawNonce)));

          // Invoke the Apple Sign in plugin
          // This will show the native Apple authentication UI
          const result = await invoke<AppleCredential>(
            "plugin:sign-in-with-apple|get_apple_id_credential",
            {
              payload: {
                scope: ["email", "fullName"],
                state,
                nonce: hashedNonce, // Send the hashed nonce to Apple
                // Disable debug mode in production
                options: {
                  debug: false
                }
              }
            }
          );

          console.log("[OAuth] Apple Sign-In result:", result);

          // Format the response for the API
          const appleUser = {
            user_identifier: result.user,
            identity_token: result.identityToken,
            email: result.email,
            given_name: result.fullName?.givenName,
            family_name: result.fullName?.familyName,
            nonce: rawNonce // Pass the original raw nonce to backend
          };

          // Send to backend via SDK
          try {
            await os.handleAppleNativeSignIn(appleUser, "");
            // Clear any existing billing token to prevent session mixing
            try {
              getBillingService().clearToken();
            } catch (billingError) {
              console.warn("Failed to clear billing token:", billingError);
            }
            // Redirect after successful login
            if (selected_plan) {
              navigate({
                to: "/pricing",
                search: { selected_plan }
              });
            } else {
              navigate({ to: next || "/" });
            }
          } catch (backendError) {
            console.error("[OAuth] Backend processing failed:", backendError);
            setError(
              backendError instanceof Error
                ? backendError.message
                : "Failed to process Apple authentication"
            );
          }
        } catch (error) {
          console.error("[OAuth] Failed to authenticate with Apple:", error);
          const errorMessage =
            error instanceof Error
              ? `Apple Sign In error: ${error.message}`
              : "Failed to authenticate with Apple. Please try again.";
          setError(errorMessage);
        }
      } else if (isTauriEnv) {
        // For Tauri desktop and Android, redirect to the web app's desktop-auth route
        let desktopAuthUrl = "https://trymaple.ai/desktop-auth?provider=apple";

        // If there's a selected plan, add it to the URL
        if (selected_plan) {
          desktopAuthUrl += `&selected_plan=${encodeURIComponent(selected_plan)}`;
        }

        // Use the opener plugin by directly invoking the command
        console.log("[OAuth] Opening URL in external browser:", desktopAuthUrl);
        invoke("plugin:opener|open_url", { url: desktopAuthUrl }).catch((error: Error) => {
          console.error("[OAuth] Failed to open external browser:", error);
          setError("Failed to open authentication page in browser");
        });
      } else {
        // Web flow - use AppleAuthProvider component which will initiate the flow
        console.log("[OAuth] Using web flow for Apple Sign In (Web only)");
        // The AppleAuthProvider component handles everything for web
        // It will be triggered by the onClick event on the button
      }
    } catch (error) {
      console.error("Failed to initiate Apple login:", error);
      setError("Failed to initiate Apple login. Please try again.");
    }
  };

  if (!loginMethod) {
    return (
      <AuthMain title="Log In" description="Choose your preferred login method">
        {error && <AlertDestructive title="Note" description={error} />}
        <Button onClick={() => setLoginMethod("email")} className="w-full">
          <Mail className="mr-2 h-4 w-4" />
          Log in with Email
        </Button>
        <Button onClick={handleGitHubLogin} className="w-full">
          <Github className="mr-2 h-4 w-4" />
          Log in with GitHub
        </Button>
        <Button onClick={handleGoogleLogin} className="w-full">
          <Google className="mr-2 h-4 w-4" />
          Log in with Google
        </Button>
        {isTauriEnv ? (
          <Button onClick={handleAppleLogin} className="w-full">
            <Apple className="mr-2 h-4 w-4" />
            Log in with Apple
          </Button>
        ) : (
          <AppleAuthProvider
            onError={(error) => setError(error.message)}
            redirectAfterLogin={(plan) => {
              if (plan) {
                navigate({ to: "/pricing", search: { selected_plan: plan } });
              } else {
                navigate({ to: next || "/" });
              }
            }}
            selectedPlan={selected_plan}
            inviteCode=""
          />
        )}
        <div className="text-center text-sm">
          Need an account?{" "}
          <Link to="/signup" search={next ? { next } : undefined} className="underline">
            Sign Up
          </Link>
        </div>
      </AuthMain>
    );
  }

  return (
    <form onSubmit={handleSubmit}>
      <AuthMain title="Log In with Email" description="Enter your email and password to log in.">
        {error && <AlertDestructive title="Error" description={error} />}
        <div className="grid gap-2">
          <Label htmlFor="email">Email</Label>
          <Input
            id="email"
            name="email"
            type="email"
            placeholder="email@example.com"
            required
            autoComplete="email"
          />
        </div>
        <div className="grid gap-2">
          <Label htmlFor="password">Password</Label>
          <Input
            id="password"
            name="password"
            type="password"
            required
            autoComplete="current-password"
          />
        </div>
        <Button type="submit" className="w-full" disabled={isLoading}>
          {isLoading ? (
            <>
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              Please wait
            </>
          ) : (
            "Log In"
          )}
        </Button>
        <Button
          type="button"
          variant="outline"
          onClick={() => setLoginMethod(null)}
          className="w-full"
        >
          Back
        </Button>
        <div className="space-y-2 mt-2">
          <div className="text-center text-sm">
            Need an account?{" "}
            <Link to="/signup" search={next ? { next } : undefined} className="underline">
              Sign up
            </Link>
          </div>
          <div className="text-center text-sm">
            <Link to="/password-reset" className="hover:underline">
              Forgot password?
            </Link>
          </div>
        </div>
      </AuthMain>
    </form>
  );
}
