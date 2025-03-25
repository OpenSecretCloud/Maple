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
          "transition-colors font-light tracking-tight text-lg",
          isActive ? "text-[#E2E2E2]" : "text-[#E2E2E2]/70 hover:text-[#E2E2E2]"
        )}
      >
        {children}
      </Link>
    );
  };

  return (
    <div className="fixed top-0 left-0 right-0 z-50 px-4 sm:px-6 lg:px-8">
      <nav className="w-full max-w-7xl mx-auto my-4">
        <div className="flex h-16 items-center px-6 relative overflow-visible rounded-xl border border-[#E2E2E2]/10 bg-[#111111]/80 backdrop-blur-md">
          <div className="relative z-10 flex w-full items-center justify-between">
            <div className="flex-shrink-0">
              <Link to="/" className="flex items-center space-x-2">
                <img src="/maple-logo.svg" alt="Maple" className="w-24" />
              </Link>
            </div>

            {/* Desktop Navigation */}
            <div className="hidden sm:flex items-center justify-between">
              <div className="flex items-center gap-6 sm:gap-10">
                <NavLink to="/pricing">Pricing</NavLink>
                <NavLink to="/proof">Proof</NavLink>
                <NavLink to="/about">About</NavLink>
              </div>
            </div>

            <div className="flex items-center gap-4">
              {/* Login/Chat Button */}
              {os.auth.user ? (
                <Button
                  onClick={() => navigate({ to: "/" })}
                  className="bg-[#9469F8] text-[#111111] hover:bg-[#A57FF9] transition-colors"
                >
                  Chat
                </Button>
              ) : (
                <Button
                  onClick={() => navigate({ to: "/login" })}
                  className="bg-[#111111] border border-[#3FDBFF]/20 text-[#E2E2E2] hover:border-[#3FDBFF]/80 transition-colors"
                >
                  Log In
                </Button>
              )}

              {/* Mobile Menu Button */}
              <button className="sm:hidden" onClick={() => setMobileMenuOpen(!mobileMenuOpen)}>
                {mobileMenuOpen ? (
                  <X className="h-6 w-6 text-[#E2E2E2]" />
                ) : (
                  <Menu className="h-6 w-6 text-[#E2E2E2]" />
                )}
              </button>
            </div>
          </div>
        </div>

        {/* Mobile Navigation Menu */}
        {mobileMenuOpen && (
          <div className="sm:hidden absolute left-4 right-4 sm:left-8 sm:right-8 mt-2 p-6 rounded-xl border border-[#E2E2E2]/10 bg-[#111111]/95 backdrop-blur-md">
            <div className="flex flex-col gap-6">
              <NavLink to="/pricing" onClick={() => setMobileMenuOpen(false)}>
                Pricing
              </NavLink>
              <NavLink to="/proof" onClick={() => setMobileMenuOpen(false)}>
                Proof
              </NavLink>
              <NavLink to="/about" onClick={() => setMobileMenuOpen(false)}>
                About
              </NavLink>
            </div>
          </div>
        )}
      </nav>
    </div>
  );
}
