import { createFileRoute, useNavigate, Link } from "@tanstack/react-router";
import { useEffect, useState, useRef, useCallback } from "react";
import { useOpenSecret } from "@opensecret/react";
import { AlertDestructive } from "@/components/AlertDestructive";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { getBillingService } from "@/billing/billingService";

export const Route = createFileRoute("/auth/$provider/callback")({
  component: OAuthCallback
});

// Define the utility function within the file
function formatProviderName(provider: string): string {
  switch (provider.toLowerCase()) {
    case "github":
      return "GitHub";
    case "google":
      return "Google";
    case "apple":
      return "Apple";
    default:
      return provider.charAt(0).toUpperCase() + provider.slice(1);
  }
}

function OAuthCallback() {
  const [isProcessing, setIsProcessing] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [redirectFailed, setRedirectFailed] = useState(false);
  const [deepLinkUrl, setDeepLinkUrl] = useState<string>("");
  const navigate = useNavigate();
  const { handleGitHubCallback, handleGoogleCallback, handleAppleCallback } = useOpenSecret();
  const processedRef = useRef(false);

  // Helper functions for the callback process
  const handleSuccessfulAuth = useCallback(() => {
    // Check if this is a Tauri app auth flow (desktop or mobile)
    const isTauriAuth = localStorage.getItem("redirect-to-native") === "true";

    // Clear the flag
    localStorage.removeItem("redirect-to-native");

    if (isTauriAuth) {
      // Handle Tauri redirect
      const accessToken = localStorage.getItem("access_token") || "";
      const refreshToken = localStorage.getItem("refresh_token");

      let deepLink = `cloud.opensecret.maple://auth?access_token=${encodeURIComponent(accessToken)}`;

      if (refreshToken) {
        deepLink += `&refresh_token=${encodeURIComponent(refreshToken)}`;
      }

      // Store the deep link for manual fallback
      setDeepLinkUrl(deepLink);

      // Set a timeout to detect if the redirect failed
      // If the user is still on this page after 5 seconds, show error
      const redirectTimer = setTimeout(() => {
        setRedirectFailed(true);
        setIsProcessing(false);
      }, 5000);

      // Attempt the redirect
      try {
        window.location.href = deepLink;

        // If we're still here after a brief moment, the redirect may have failed
        // The timer above will catch this
      } catch (error) {
        // Clear the timer and show error immediately if redirect throws
        clearTimeout(redirectTimer);
        setRedirectFailed(true);
        setIsProcessing(false);
        console.error("Failed to redirect to app:", error);
      }

      return;
    }

    // Handle web redirect
    const selectedPlan = sessionStorage.getItem("selected_plan");
    sessionStorage.removeItem("selected_plan");

    setTimeout(() => {
      if (selectedPlan) {
        navigate({
          to: "/pricing",
          search: { selected_plan: selectedPlan }
        });
      } else {
        navigate({ to: "/" });
      }
    }, 2000);
  }, [navigate, setDeepLinkUrl, setIsProcessing, setRedirectFailed]);

  const handleAuthError = (error: unknown) => {
    console.error(`Authentication callback error:`, error);
    if (error instanceof Error) {
      setError(error.message);
    } else {
      setError("Unknown error");
    }
    setIsProcessing(false);
  };

  const { provider } = Route.useParams();
  const formattedProvider = formatProviderName(provider); // Format the provider name

  useEffect(() => {
    const processCallback = async () => {
      if (processedRef.current) return;
      processedRef.current = true;

      // Get URL parameters for all OAuth providers
      const urlParams = new URLSearchParams(window.location.search);
      const code = urlParams.get("code");
      const state = urlParams.get("state");

      // For Apple, we might get form data instead of URL parameters
      // Apple uses form_post with POST request in some scenarios
      let appleData = null;
      if (provider === "apple" && !code) {
        // Check if we have Apple data in sessionStorage from form_post
        const appleFormData = sessionStorage.getItem("apple_form_data");
        if (appleFormData) {
          try {
            appleData = JSON.parse(appleFormData);
            sessionStorage.removeItem("apple_form_data");
          } catch (e) {
            console.error("Failed to parse Apple form data:", e);
          }
        }
      }

      if ((code && state) || (provider === "apple" && appleData)) {
        try {
          // Handle the callback based on the provider
          if (provider === "github") {
            await handleGitHubCallback(code || "", state || "", "");
          } else if (provider === "google") {
            await handleGoogleCallback(code || "", state || "", "");
          } else if (provider === "apple") {
            // This handles the redirect flow (backup for non-popup scenarios)
            // Most Apple auth will now be handled client-side in the AppleAuthProvider component
            await handleAppleCallback(code || "", state || "", "");
          } else {
            throw new Error(`Unsupported provider: ${provider}`);
          }

          // Clear any existing billing token to prevent session mixing
          try {
            getBillingService().clearToken();
          } catch (billingError) {
            console.warn("Failed to clear billing token:", billingError);
          }

          // Handle the successful authentication (redirect)
          handleSuccessfulAuth();
        } catch (error) {
          // Handle authentication error
          handleAuthError(error);
        } finally {
          setIsProcessing(false);
        }
      } else {
        setError("Invalid callback parameters");
        setIsProcessing(false);
      }
    };

    processCallback();
  }, [
    handleGitHubCallback,
    handleGoogleCallback,
    handleAppleCallback,
    handleSuccessfulAuth,
    navigate,
    provider
  ]);

  // If this is a Tauri app auth flow (desktop or mobile), show a different UI
  if (localStorage.getItem("redirect-to-native") === "true" || redirectFailed) {
    if (redirectFailed) {
      return (
        <Card className="max-w-md mx-auto mt-20">
          <CardHeader>
            <CardTitle>Redirect Failed</CardTitle>
          </CardHeader>
          <CardContent>
            <AlertDestructive
              title="Unable to redirect to app"
              description="The automatic redirect to the Maple app failed. Please try one of the options below."
            />
            <div className="mt-4 space-y-3">
              <p className="text-sm text-muted-foreground">
                If the Maple app is installed, click the button below to manually open it:
              </p>
              <Button
                onClick={() => {
                  window.location.href = deepLinkUrl;
                }}
                className="w-full"
              >
                Open Maple App
              </Button>
              <p className="text-sm text-muted-foreground">
                Or copy this link and paste it into your browser:
              </p>
              <div className="bg-muted p-2 rounded text-xs break-all font-mono">{deepLinkUrl}</div>
              <div className="pt-2">
                <Button variant="outline" asChild className="w-full">
                  <Link to="/login">Back to Login</Link>
                </Button>
              </div>
            </div>
          </CardContent>
        </Card>
      );
    }

    return (
      <Card className="max-w-md mx-auto mt-20">
        <CardHeader>
          <CardTitle>{formattedProvider} Authentication Successful</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="mb-4">Authentication successful! Redirecting you back to the app...</p>
          <div className="flex justify-center">
            <Loader2 className="h-8 w-8 animate-spin" />
          </div>
        </CardContent>
      </Card>
    );
  }

  // Regular processing UI for web flow
  if (isProcessing) {
    return (
      <Card className="max-w-md mx-auto mt-20">
        <CardHeader>
          <CardTitle>Processing {formattedProvider} Login</CardTitle>
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
          <CardTitle>Authentication Failed</CardTitle>
        </CardHeader>
        <CardContent>
          <AlertDestructive title="Error" description={error} />
          <div className="mt-4 flex justify-center">
            <Button asChild>
              <Link to="/">Try Again</Link>
            </Button>
          </div>
        </CardContent>
      </Card>
    );
  }

  return (
    <Card className="max-w-md mx-auto mt-20">
      <CardHeader>
        <CardTitle>{formattedProvider} Authentication Successful</CardTitle>
      </CardHeader>
      <CardContent>
        You have successfully authenticated with {formattedProvider}.
        {sessionStorage.getItem("selected_plan")
          ? "Redirecting to complete your plan selection..."
          : "Redirecting to home page..."}
      </CardContent>
    </Card>
  );
}
