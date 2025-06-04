import { createFileRoute, useNavigate } from "@tanstack/react-router";
import { useEffect } from "react";
import { useOpenSecret } from "@opensecret/react";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Loader2 } from "lucide-react";
import { AppleAuthProvider } from "@/components/AppleAuthProvider";

// Define the search parameters interface
interface DesktopAuthSearchParams {
  provider: string;
  selected_plan?: string;
}

// This route handles OAuth flow for both desktop and mobile Tauri apps
export const Route = createFileRoute("/desktop-auth")({
  component: DesktopAuth,
  validateSearch: (search: Record<string, unknown>): DesktopAuthSearchParams => {
    const provider = typeof search.provider === "string" ? search.provider : "github";
    // Validate provider is supported
    if (provider !== "github" && provider !== "google" && provider !== "apple") {
      throw new Error(`Unsupported provider: ${provider}`);
    }
    return {
      provider,
      selected_plan: typeof search.selected_plan === "string" ? search.selected_plan : undefined
    };
  }
});

function DesktopAuth() {
  // Use the typed search params
  const search = Route.useSearch();
  const { provider, selected_plan } = search;
  const navigate = useNavigate();
  const os = useOpenSecret();

  useEffect(() => {
    const initiateAuth = async () => {
      try {
        // Store the flag to indicate this is a Tauri app auth flow (desktop or mobile)
        localStorage.setItem("redirect-to-native", "true");

        // Store selected plan if present
        if (selected_plan) {
          sessionStorage.setItem("selected_plan", selected_plan);
        }

        // For Apple, we don't need to do anything here - the AppleAuthProvider
        // component will handle the authentication flow with popup
        if (provider === "apple") {
          return;
        }

        // Initiate appropriate OAuth flow for GitHub and Google
        let auth_url;
        if (provider === "github") {
          const result = await os.initiateGitHubAuth("");
          auth_url = result.auth_url;
        } else if (provider === "google") {
          const result = await os.initiateGoogleAuth("");
          auth_url = result.auth_url;
        } else {
          throw new Error("Unsupported provider");
        }

        // Redirect to the OAuth provider
        window.location.href = auth_url;
      } catch (error) {
        console.error(`Failed to initiate ${provider} login:`, error);
        // Redirect to login page on error
        navigate({ to: "/login" });
      }
    };

    initiateAuth();
  }, [os, provider, selected_plan, navigate]);

  // Special handling for Apple OAuth - use popup instead of redirect
  if (provider === "apple") {
    return (
      <Card className="max-w-md mx-auto mt-20">
        <CardHeader>
          <CardTitle>Apple Sign In</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="mb-4">Click the button below to sign in with Apple:</p>
          <AppleAuthProvider
            onError={(error) => {
              console.error("Apple auth error:", error);
              navigate({ to: "/login" });
            }}
            redirectAfterLogin={(plan) => {
              if (plan) {
                navigate({ to: "/pricing", search: { selected_plan: plan } });
              } else {
                navigate({ to: "/" });
              }
            }}
            selectedPlan={selected_plan}
            inviteCode=""
          />
        </CardContent>
      </Card>
    );
  }

  return (
    <Card className="max-w-md mx-auto mt-20">
      <CardHeader>
        <CardTitle>Redirecting to {provider}</CardTitle>
      </CardHeader>
      <CardContent>
        <p className="mb-4">Please wait while we redirect you to complete authentication...</p>
        <div className="flex justify-center">
          <Loader2 className="h-8 w-8 animate-spin" />
        </div>
      </CardContent>
    </Card>
  );
}
