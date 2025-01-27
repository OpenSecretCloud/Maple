import { createFileRoute, Link } from "@tanstack/react-router";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { useState } from "react";
import { Loader2, CheckCircle } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { AlertDestructive } from "@/components/AlertDestructive";

export const Route = createFileRoute("/waitlist")({
  component: WaitlistPage
});

function WaitlistPage() {
  const [error, setError] = useState<string | null>(null);
  const [isLoading, setIsLoading] = useState(false);
  const [submitStatus, setSubmitStatus] = useState<string | null>(null);

  async function handleWaitlistSubmit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setIsLoading(true);
    setError(null);
    setSubmitStatus(null);

    const target = event.target as HTMLFormElement;
    const data = new FormData(target);

    try {
      const response = await fetch(target.action, {
        method: target.method,
        body: data,
        headers: {
          Accept: "application/json"
        }
      });

      if (response.ok) {
        console.log("Form submitted successfully");
        setSubmitStatus("Thank you! We'll be in touch.");
      } else {
        console.error("Form submission failed");
        setError("Form submission failed. Please try again.");
      }
    } catch (error) {
      console.error(`Form submission failed: ${error}`);
      setError("An error occurred. Please try again later.");
    } finally {
      setIsLoading(false);
    }
  }

  return (
    <div className="pt-8 mx-auto max-w-md">
      <form onSubmit={handleWaitlistSubmit} action="https://formspree.io/f/xnnqqkvp" method="POST">
        <Card className="bg-card/70 backdrop-blur-sm mx-auto max-w-[45rem]">
          <CardHeader className="pb-4">
            <CardTitle className="text-xl">Join the Waitlist</CardTitle>
            <CardDescription>
              Enter your email to join our waitlist and get notified when we launch.
            </CardDescription>
          </CardHeader>
          <CardContent className="grid gap-4 pt-0">
            {error && <AlertDestructive title="Error" description={error} />}
            {submitStatus ? (
              <Alert className="bg-background">
                <CheckCircle className="h-4 w-4" />
                <AlertTitle>Submitted</AlertTitle>
                <AlertDescription>{submitStatus}</AlertDescription>
              </Alert>
            ) : (
              <>
                <div className="grid gap-2">
                  <Label htmlFor="waitlist-email">Email</Label>
                  <Input
                    id="waitlist-email"
                    name="email"
                    type="email"
                    placeholder="email@example.com"
                    required
                    autoComplete="email"
                  />
                </div>
                <Button type="submit" className="w-full" disabled={isLoading}>
                  {isLoading ? (
                    <>
                      <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                      Please wait
                    </>
                  ) : (
                    "Join Waitlist"
                  )}
                </Button>
              </>
            )}
            <div className="text-center text-sm">
              Already have an invite code?{" "}
              <Link to="/signup" className="font-medium hover:underline">
                Sign up
              </Link>
            </div>
          </CardContent>
        </Card>
      </form>
    </div>
  );
}
