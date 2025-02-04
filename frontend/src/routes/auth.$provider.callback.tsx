import { createFileRoute, useNavigate, Link } from "@tanstack/react-router";
import { useEffect, useState, useRef } from "react";
import { useOpenSecret } from "@opensecret/react";
import { AlertDestructive } from "@/components/AlertDestructive";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";

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
    default:
      return provider.charAt(0).toUpperCase() + provider.slice(1);
  }
}

function OAuthCallback() {
  const [isProcessing, setIsProcessing] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const navigate = useNavigate();
  const { handleGitHubCallback, handleGoogleCallback } = useOpenSecret();
  const processedRef = useRef(false);

  const { provider } = Route.useParams();
  const formattedProvider = formatProviderName(provider); // Format the provider name

  useEffect(() => {
    const processCallback = async () => {
      if (processedRef.current) return;
      processedRef.current = true;

      const urlParams = new URLSearchParams(window.location.search);
      const code = urlParams.get("code");
      const state = urlParams.get("state");

      if (code && state) {
        try {
          if (provider === "github") {
            await handleGitHubCallback(code, state, "");
          } else if (provider === "google") {
            await handleGoogleCallback(code, state, "");
          } else {
            throw new Error("Unsupported provider");
          }

          // Check for stored selected_plan
          const selectedPlan = sessionStorage.getItem("selected_plan");
          // Clear it from storage
          sessionStorage.removeItem("selected_plan");

          // If successful, redirect after a short delay
          setTimeout(() => {
            if (selectedPlan) {
              // If there was a selected plan, go to pricing
              navigate({
                to: "/pricing",
                search: { selected_plan: selectedPlan }
              });
            } else {
              // Otherwise go home (original behavior)
              navigate({ to: "/" });
            }
          }, 2000);
        } catch (error) {
          console.error(`${provider} callback error:`, error);
          if (error instanceof Error) {
            setError(error.message);
          } else {
            setError("Unknown error");
          }
        } finally {
          setIsProcessing(false);
        }
      } else {
        setError("Invalid callback parameters");
        setIsProcessing(false);
      }
    };

    processCallback();
  }, [handleGitHubCallback, handleGoogleCallback, navigate, provider]);

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
