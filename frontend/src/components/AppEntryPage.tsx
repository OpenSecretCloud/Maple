import { Link } from "@tanstack/react-router";
import { LogIn, UserPlus } from "lucide-react";
import { AuthHeader } from "@/components/AuthHeader";
import { Button } from "@/components/ui/button";

export function AppEntryPage() {
  return (
    <div className="flex min-h-dvh flex-col bg-[#e2e2e2] text-[#221a18] dark:bg-background dark:text-foreground">
      <AuthHeader />

      <main className="flex flex-1 items-center justify-center px-4 pb-16 pt-8 sm:px-6">
        <section className="w-full max-w-md rounded-lg border border-neutral-900/10 bg-white/75 p-5 text-center shadow-sm backdrop-blur dark:border-white/10 dark:bg-neutral-900/70 sm:p-6">
          <img src="/maple-research-icon.svg" alt="" className="mx-auto mb-4 h-14 w-14" />
          <h1 className="text-2xl font-semibold leading-tight text-[#221a18] dark:text-foreground">
            Maple Research
          </h1>
          <p className="mt-2 text-sm leading-6 text-[#747474] dark:text-muted-foreground">
            Sign up or log in to continue.
          </p>

          <div className="mt-6 grid gap-3">
            <Button asChild variant="primary" size="lg" className="h-12">
              <Link to="/signup">
                <UserPlus className="mr-2 h-4 w-4" />
                Sign up
              </Link>
            </Button>
            <Button
              asChild
              variant="outline"
              size="lg"
              className="h-12 bg-white/40 dark:bg-white/0"
            >
              <Link to="/login">
                <LogIn className="mr-2 h-4 w-4" />
                Log in
              </Link>
            </Button>
          </div>
        </section>
      </main>
    </div>
  );
}
