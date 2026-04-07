import { Link, useMatchRoute, useNavigate } from "@tanstack/react-router";
import { cn } from "@/utils/utils";
import { Button } from "./ui/button";
import { useOpenSecret } from "@opensecret/react";
import { ChevronDown, Menu, X } from "lucide-react";
import { useState } from "react";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger
} from "@/components/ui/dropdown-menu";

const SOLUTION_ITEMS = [
  { to: "/solutions", label: "All solutions" },
  { to: "/solutions/lawyers", label: "AI for Lawyers" },
  { to: "/solutions/accountants", label: "AI for Accountants" },
  { to: "/solutions/finance", label: "AI for Finance" },
  { to: "/solutions/therapy", label: "AI for Therapy" },
  { to: "/solutions/teams", label: "AI for Teams" },
  { to: "/solutions/developers", label: "Secure API for Developers" }
] as const;

export function TopNav() {
  const os = useOpenSecret();
  const navigate = useNavigate();
  const matchRoute = useMatchRoute();
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);

  const solutionsActive = !!matchRoute({ to: "/solutions", fuzzy: true });

  const NavLink = ({
    to,
    children,
    onClick,
    fuzzy
  }: {
    to: string;
    children: React.ReactNode;
    onClick?: () => void;
    fuzzy?: boolean;
  }) => {
    const isActive = !!matchRoute({ to, fuzzy: fuzzy ?? false });
    return (
      <Link
        to={to}
        onClick={onClick}
        className={cn(
          "transition-colors font-light tracking-tight text-base lg:text-lg",
          isActive ? "text-marketingNav-fg" : "text-marketingNav-fg/70 hover:text-marketingNav-fg"
        )}
      >
        {children}
      </Link>
    );
  };

  const closeMobile = () => setMobileMenuOpen(false);

  return (
    <div className="fixed top-0 left-0 right-0 z-50 px-4 sm:px-6 lg:px-8">
      <nav className="w-full max-w-7xl mx-auto my-4">
        <div className="relative flex h-16 items-center overflow-visible rounded-xl border border-marketingNav-fg/10 bg-marketingNav-bg/80 px-4 sm:px-6 backdrop-blur-md">
          <div className="relative z-10 flex w-full items-center justify-between gap-2">
            <div className="flex-shrink-0 min-w-0">
              <Link to="/" className="flex items-center gap-2">
                <img src="/maple-icon-nobg.png" alt="" className="h-8 w-8 shrink-0" />
                <img src="/maple-logo.svg" alt="Maple" className="w-20 sm:w-24 h-auto" />
              </Link>
            </div>

            <div className="hidden lg:flex items-center justify-center flex-1 min-w-0">
              <div className="flex items-center gap-4 xl:gap-6 flex-wrap justify-center">
                <NavLink to="/agent">Agent</NavLink>
                <NavLink to="/research">Research</NavLink>
                <DropdownMenu>
                  <DropdownMenuTrigger
                    className={cn(
                      "inline-flex items-center gap-1 rounded-md font-light tracking-tight text-base xl:text-lg outline-none transition-colors",
                      solutionsActive
                        ? "text-marketingNav-fg"
                        : "text-marketingNav-fg/70 hover:text-marketingNav-fg"
                    )}
                  >
                    Solutions
                    <ChevronDown className="h-4 w-4 opacity-70" aria-hidden />
                  </DropdownMenuTrigger>
                  <DropdownMenuContent
                    align="center"
                    className="min-w-[14rem] border-marketingNav-fg/10 bg-marketingNav-bg/95 backdrop-blur-md"
                  >
                    {SOLUTION_ITEMS.map(({ to, label }) => (
                      <DropdownMenuItem key={to} asChild>
                        <Link to={to}>{label}</Link>
                      </DropdownMenuItem>
                    ))}
                  </DropdownMenuContent>
                </DropdownMenu>
                <NavLink to="/pricing">Pricing</NavLink>
                <NavLink to="/proof">Proof</NavLink>
              </div>
            </div>

            <div className="flex items-center gap-2 sm:gap-4 shrink-0">
              {os.auth.user ? (
                <Button
                  onClick={() => navigate({ to: "/" })}
                  className="bg-[hsl(var(--maple-primary))] text-[hsl(var(--maple-on-primary))] hover:bg-[hsl(var(--maple-primary))]/80 transition-colors"
                >
                  Chat
                </Button>
              ) : (
                <Button
                  onClick={() => navigate({ to: "/login" })}
                  className="border border-[hsl(var(--blue))]/20 bg-marketingNav-bg text-marketingNav-fg transition-colors hover:border-[hsl(var(--blue))]/80 hover:bg-marketingNav-bg"
                >
                  Log In
                </Button>
              )}

              <button
                type="button"
                className="lg:hidden p-2 -mr-2"
                aria-expanded={mobileMenuOpen}
                aria-label={mobileMenuOpen ? "Close menu" : "Open menu"}
                onClick={() => setMobileMenuOpen(!mobileMenuOpen)}
              >
                {mobileMenuOpen ? (
                  <X className="h-6 w-6 text-marketingNav-fg" />
                ) : (
                  <Menu className="h-6 w-6 text-marketingNav-fg" />
                )}
              </button>
            </div>
          </div>
        </div>

        {mobileMenuOpen && (
          <div className="absolute left-4 right-4 mt-2 max-h-[min(70vh,calc(100dvh-8rem))] overflow-y-auto rounded-xl border border-marketingNav-fg/10 bg-marketingNav-bg/95 p-6 backdrop-blur-md lg:hidden">
            <div className="flex flex-col gap-5">
              <NavLink to="/agent" onClick={closeMobile}>
                Agent
              </NavLink>
              <NavLink to="/research" onClick={closeMobile}>
                Research
              </NavLink>
              <div className="flex flex-col gap-3 border-t border-marketingNav-fg/10 pt-4">
                <span className="text-xs font-medium uppercase tracking-wide text-marketingNav-fg/50">
                  Solutions
                </span>
                {SOLUTION_ITEMS.map(({ to, label }) => (
                  <NavLink key={to} to={to} onClick={closeMobile}>
                    {label}
                  </NavLink>
                ))}
              </div>
              <div className="border-t border-marketingNav-fg/10 pt-4" />
              <NavLink to="/pricing" onClick={closeMobile}>
                Pricing
              </NavLink>
              <NavLink to="/proof" onClick={closeMobile}>
                Proof
              </NavLink>
            </div>
          </div>
        )}
      </nav>
    </div>
  );
}
