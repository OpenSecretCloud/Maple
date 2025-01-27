import { Link, useMatchRoute, useNavigate } from "@tanstack/react-router";
import { cn } from "@/utils/utils";
import { Button } from "./ui/button";
import { useOpenSecret } from "@opensecret/react";
import { Menu, X } from "lucide-react";
import { useState } from "react";

export function TopNav() {
  const os = useOpenSecret();
  const navigate = useNavigate();
  const matchRoute = useMatchRoute();
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);

  const NavLink = ({
    to,
    children,
    onClick
  }: {
    to: string;
    children: React.ReactNode;
    onClick?: () => void;
  }) => {
    const isActive = matchRoute({ to });
    return (
      <Link
        to={to}
        onClick={onClick}
        className={cn(
          "transition-colors font-light tracking-tight text-xl text-white",
          isActive ? "text-white" : "text-white/80 hover:text-white"
        )}
      >
        {children}
      </Link>
    );
  };

  return (
    <div className="flex justify-center px-4 sm:px-8 fixed top-0 left-0 right-0 z-50">
      <nav className="w-full my-4 rounded-lg">
        <div className="flex h-16 items-center px-4 sm:px-6 relative overflow-visible rounded-lg border bg-black/75 border-white/10">
          <div className="relative z-10 flex w-full items-center justify-between">
            <div className="mr-4 flex-shrink-0">
              <Link to="/" className="flex items-center space-x-2">
                <img src="/maple-logo.svg" alt="Maple" className="w-24" />
              </Link>
            </div>

            {/* Desktop Navigation */}
            <div className="hidden sm:flex items-center justify-between">
              <div className="flex items-center gap-4 sm:gap-8">
                <NavLink to="/pricing">Pricing</NavLink>
                <NavLink to="/proof">Proof</NavLink>
                <NavLink to="/about">About</NavLink>
              </div>
            </div>

            <div className="flex items-center gap-4">
              {/* Waitlist Link (Desktop) */}
              <div className="hidden sm:block">
                <NavLink to="/waitlist">Join Waitlist</NavLink>
              </div>

              {/* Login/Chat Button */}
              {os.auth.user ? (
                <Button variant="secondary" onClick={() => navigate({ to: "/" })}>
                  Chat
                </Button>
              ) : (
                <Button variant="default" onClick={() => navigate({ to: "/login" })}>
                  Log In
                </Button>
              )}

              {/* Mobile Menu Button */}
              <button className="sm:hidden" onClick={() => setMobileMenuOpen(!mobileMenuOpen)}>
                {mobileMenuOpen ? (
                  <X className="h-6 w-6 text-white" />
                ) : (
                  <Menu className="h-6 w-6 text-white" />
                )}
              </button>
            </div>
          </div>
        </div>

        {/* Mobile Navigation Menu */}
        {mobileMenuOpen && (
          <div className="sm:hidden absolute left-4 right-4 sm:left-8 sm:right-8 mt-2 p-4 rounded-lg border bg-black/75 border-white/10 backdrop-blur-sm">
            <div className="flex flex-col gap-4">
              <NavLink to="/pricing" onClick={() => setMobileMenuOpen(false)}>
                Pricing
              </NavLink>
              <NavLink to="/proof" onClick={() => setMobileMenuOpen(false)}>
                Proof
              </NavLink>
              <NavLink to="/about" onClick={() => setMobileMenuOpen(false)}>
                About
              </NavLink>
              <NavLink to="/waitlist" onClick={() => setMobileMenuOpen(false)}>
                Join Waitlist
              </NavLink>
            </div>
          </div>
        )}
      </nav>
    </div>
  );
}
