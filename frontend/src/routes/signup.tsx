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
};

export const Route = createFileRoute("/signup")({
  component: SignupPage,
  validateSearch: (search: Record<string, unknown>): SignupSearchParams => ({
    next: typeof search.next === "string" ? search.next : undefined
  })
});

type SignUpMethod = "email" | "github" | "google" | null;

function SignupPage() {
  const navigate = useNavigate();
  const os = useOpenSecret();
  const { next } = Route.useSearch();
  const [signUpMethod, setSignUpMethod] = useState<SignUpMethod>(null);
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);

  // Redirect if already logged in
  useEffect(() => {
    if (os.auth.user) {
      navigate({ to: next || "/" });
    }
  }, [os.auth.user, navigate, next]);

  const handleSubmit = async (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault();
    setIsLoading(true);
    setError(null);
    const formData = new FormData(e.currentTarget);
    const email = formData.get("email") as string;
    const password = formData.get("password") as string;
    const inviteCode = formData.get("inviteCode") as string;

    try {
      await os.signUp(email, password, inviteCode, "ANON");
      setTimeout(() => {
        navigate({ to: next || "/" });
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
      const inviteCode = (document.getElementById("inviteCode") as HTMLInputElement)?.value;
      if (!inviteCode) {
        setError("Invite code is required");
        return;
      }
      const { auth_url } = await os.initiateGitHubAuth(inviteCode);
      window.localStorage.setItem("github_invite_code", inviteCode);
      window.location.href = auth_url;
    } catch (error) {
      console.error("Failed to initiate GitHub signup:", error);
      if (error instanceof Error && error.message.includes("Invalid invite code")) {
        setError("Invalid invite code. Please check and try again.");
      } else {
        setError("Failed to initiate GitHub signup. Please try again.");
      }
    }
  };

  const handleGoogleSignup = async () => {
    try {
      const inviteCode = (document.getElementById("inviteCode") as HTMLInputElement)?.value;
      if (!inviteCode) {
        setError("Invite code is required");
        return;
      }
      const { auth_url } = await os.initiateGoogleAuth(inviteCode);
      window.localStorage.setItem("google_invite_code", inviteCode);
      window.location.href = auth_url;
    } catch (error) {
      console.error("Failed to initiate Google signup:", error);
      if (error instanceof Error && error.message.includes("Invalid invite code")) {
        setError("Invalid invite code. Please check and try again.");
      } else {
        setError("Failed to initiate Google signup. Please try again.");
      }
    }
  };

  if (!signUpMethod) {
    return (
      <AuthMain title="Sign Up" description="Choose your preferred sign-up method">
        <Button onClick={() => setSignUpMethod("email")} className="w-full">
          <Mail className="mr-2 h-4 w-4" />
          Sign up with Email
        </Button>
        <Button onClick={() => setSignUpMethod("github")} className="w-full">
          <Github className="mr-2 h-4 w-4" />
          Sign up with GitHub
        </Button>
        <Button onClick={() => setSignUpMethod("google")} className="w-full">
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

  if (signUpMethod === "github") {
    return (
      <form
        onSubmit={(e) => {
          e.preventDefault();
          handleGitHubSignup();
        }}
      >
        <AuthMain title="Sign Up with GitHub" description="Invite code required for beta access.">
          {error && <AlertDestructive title="Error" description={error} />}
          <div className="grid gap-2">
            <Label htmlFor="inviteCode">Invite Code</Label>
            <Input id="inviteCode" name="inviteCode" type="text" required />
          </div>
          <Button type="submit" className="w-full">
            <Github className="mr-2 h-4 w-4" />
            Continue with GitHub
          </Button>
          <Button
            type="button"
            variant="outline"
            onClick={() => setSignUpMethod(null)}
            className="w-full"
          >
            Back
          </Button>
        </AuthMain>
      </form>
    );
  }

  if (signUpMethod === "google") {
    return (
      <form
        onSubmit={(e) => {
          e.preventDefault();
          handleGoogleSignup();
        }}
      >
        <AuthMain title="Sign Up with Google" description="Invite code required for beta access.">
          {error && <AlertDestructive title="Error" description={error} />}
          <div className="grid gap-2">
            <Label htmlFor="inviteCode">Invite Code</Label>
            <Input id="inviteCode" name="inviteCode" type="text" required />
          </div>
          <Button type="submit" className="w-full">
            <Google className="mr-2 h-4 w-4" />
            Continue with Google
          </Button>
          <Button
            type="button"
            variant="outline"
            onClick={() => setSignUpMethod(null)}
            className="w-full"
          >
            Back
          </Button>
        </AuthMain>
      </form>
    );
  }

  return (
    <form onSubmit={handleSubmit}>
      <AuthMain title="Sign Up with Email" description="Invite code required for beta access.">
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
        <div className="grid gap-2">
          <Label htmlFor="inviteCode">Invite Code</Label>
          <Input id="inviteCode" name="inviteCode" type="text" required />
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
