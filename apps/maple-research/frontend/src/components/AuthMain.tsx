import { Link } from "@tanstack/react-router";
import { AuthHeader } from "@/components/AuthHeader";

type AuthMainProps = {
  children: React.ReactNode;
  title: string;
  description?: string;
};

export function AuthMain({ children, title, description }: AuthMainProps) {
  return (
    <div className="flex min-h-dvh flex-col bg-[#e2e2e2] text-[#221a18] dark:bg-background dark:text-foreground">
      <AuthHeader />

      <main className="flex flex-1 items-center justify-center px-4 py-10 sm:px-6">
        <section className="w-full max-w-md rounded-lg border border-neutral-900/10 bg-white/75 p-5 shadow-sm backdrop-blur dark:border-white/10 dark:bg-neutral-900/70 sm:p-6">
          <div className="mb-5">
            <h1 className="text-2xl font-semibold leading-tight text-[#221a18] dark:text-foreground">
              {title}
            </h1>
            {description && (
              <p className="mt-2 text-sm leading-6 text-[#747474] dark:text-muted-foreground">
                {description}
              </p>
            )}
          </div>
          <div className="grid gap-4">{children}</div>
        </section>
      </main>

      <footer className="px-4 pb-6 text-center text-xs text-[#747474] dark:text-muted-foreground">
        <div className="flex flex-wrap items-center justify-center gap-x-4 gap-y-2">
          <span>© {new Date().getFullYear()} Maple Privacy Labs Inc.</span>
          <Link to="/terms" className="hover:text-[#221a18] dark:hover:text-foreground">
            Terms
          </Link>
          <Link to="/privacy" className="hover:text-[#221a18] dark:hover:text-foreground">
            Privacy
          </Link>
        </div>
      </footer>
    </div>
  );
}
