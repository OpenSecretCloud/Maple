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

type SignupSearchParams = {
  next?: string;
  selected_plan?: string;
};

export const Route = createFileRoute("/signup")({
  component: SignupPage,
  validateSearch: (search: Record<string, unknown>): SignupSearchParams => ({
    next: typeof search.next === "string" ? search.next : undefined,
    selected_plan: typeof search.selected_plan === "string" ? search.selected_plan : undefined
  })
});

type SignUpMethod = "email" | "github" | "google" | null;

function SignupPage() {
  const navigate = useNavigate();
  const os = useOpenSecret();
  const { next, selected_plan } = Route.useSearch();
  const [signUpMethod, setSignUpMethod] = useState<SignUpMethod>(null);
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
      await os.signUp(email, password, "", "ANON");
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

  const handleGitHubSignup = async () => {
    try {
      const { auth_url } = await os.initiateGitHubAuth("");
      if (selected_plan) {
        sessionStorage.setItem("selected_plan", selected_plan);
      }
      window.location.href = auth_url;
    } catch (error) {
      console.error("Failed to initiate GitHub signup:", error);
      setError("Failed to initiate GitHub signup. Please try again.");
    }
  };

  const handleGoogleSignup = async () => {
    try {
      const { auth_url } = await os.initiateGoogleAuth("");
      if (selected_plan) {
        sessionStorage.setItem("selected_plan", selected_plan);
      }
      window.location.href = auth_url;
    } catch (error) {
      console.error("Failed to initiate Google signup:", error);
      setError("Failed to initiate Google signup. Please try again.");
    }
  };

  if (!signUpMethod) {
    return (
      <AuthMain title="Sign Up" description="Choose your preferred sign-up method">
        <Button onClick={() => setSignUpMethod("email")} className="w-full">
          <Mail className="mr-2 h-4 w-4" />
          Sign up with Email
        </Button>
        <Button onClick={handleGitHubSignup} className="w-full">
          <Github className="mr-2 h-4 w-4" />
          Sign up with GitHub
        </Button>
        <Button onClick={handleGoogleSignup} className="w-full">
          <Google className="mr-2 h-4 w-4" />
          Sign up with Google
        </Button>
        <div className="text-center text-sm">
          Already have an account?{" "}
          <Link to="/login" search={next ? { next } : undefined} className="underline">
            Log In
          </Link>
        </div>
      </AuthMain>
    );
  }

  return (
    <form onSubmit={handleSubmit}>
      <AuthMain title="Sign Up with Email">
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
            minLength={8}
            autoComplete="new-password"
          />
        </div>
        <Button type="submit" className="w-full" disabled={isLoading}>
          {isLoading ? (
            <>
              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
              Please wait
            </>
          ) : (
            "Create Account"
          )}
        </Button>
        <Button
          type="button"
          variant="outline"
          onClick={() => setSignUpMethod(null)}
          className="w-full"
        >
          Back
        </Button>
        <div className="text-center text-sm">
          Already have an account?{" "}
          <Link to="/login" search={next ? { next } : undefined} className="underline">
            Log In
          </Link>
        </div>
      </AuthMain>
    </form>
  );
}
