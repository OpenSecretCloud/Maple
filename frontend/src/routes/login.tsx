import { createFileRoute, Link, useNavigate } from "@tanstack/react-router";
import { useOpenSecret } from "@opensecret/react";
import { useEffect, useState } from "react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { AlertDestructive } from "@/components/AlertDestructive";
import { Loader2, Github, Mail } from "lucide-react";
import { Google } from "@/components/icons/Google";
import { AuthMain } from "@/components/AuthMain";

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

type LoginMethod = "email" | "github" | "google" | null;

function LoginPage() {
  const navigate = useNavigate();
  const os = useOpenSecret();
  const { next, selected_plan } = Route.useSearch();
  const [loginMethod, setLoginMethod] = useState<LoginMethod>(null);
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);

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
      const { auth_url } = await os.initiateGitHubAuth("");
      if (selected_plan) {
        sessionStorage.setItem("selected_plan", selected_plan);
      }
      window.location.href = auth_url;
    } catch (error) {
      console.error("Failed to initiate GitHub login:", error);
      setError("Failed to initiate GitHub login. Please try again.");
    }
  };

  const handleGoogleLogin = async () => {
    try {
      const { auth_url } = await os.initiateGoogleAuth("");
      if (selected_plan) {
        sessionStorage.setItem("selected_plan", selected_plan);
      }
      window.location.href = auth_url;
    } catch (error) {
      console.error("Failed to initiate Google login:", error);
      setError("Failed to initiate Google login. Please try again.");
    }
  };

  if (!loginMethod) {
    return (
      <AuthMain title="Log In" description="Choose your preferred login method">
        <Button onClick={() => setLoginMethod("email")} className="w-full" style={{ backgroundColor: '#E2E2E2' }}>
          <Mail className="mr-2 h-4 w-4" />
          Log in with Email
        </Button>
        <Button onClick={handleGitHubLogin} className="w-full" style={{ backgroundColor: '#E2E2E2' }}>
          <Github className="mr-2 h-4 w-4" />
          Log in with GitHub
        </Button>
        <Button onClick={handleGoogleLogin} className="w-full" style={{ backgroundColor: '#E2E2E2' }}>
          <Google className="mr-2 h-4 w-4" />
          Log in with Google
        </Button>
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
