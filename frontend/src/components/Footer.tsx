import { Github, Twitter, Mail } from "lucide-react";
import { Link } from "@tanstack/react-router";
import { DiscordIcon } from "./icons/DiscordIcon";
import { isTauri } from "@/utils/platform";

const footerLink =
  "text-[hsl(var(--marketing-text-muted))] hover:text-foreground transition-colors font-light";

export function Footer() {
  const isTauriPlatform = isTauri();

  return (
    <div className="w-full dark:bg-[hsl(var(--background))] bg-[hsl(var(--footer-bg))] py-16 border-t border-[hsl(var(--marketing-card-border))]">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
        <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-6 gap-10 lg:gap-8">
          <div className="lg:col-span-2 flex flex-col gap-4">
            <img src="/maple-logo-dark.svg" alt="Maple" className="w-32 block dark:hidden" />
            <img src="/maple-logo.svg" alt="Maple" className="w-32 hidden dark:block" />
            <p className="text-[hsl(var(--marketing-text-muted))] font-light max-w-sm">
              The AI Platform for Privileged Information. Your data stays yours with end-to-end
              encryption.
            </p>
            <div className="flex gap-5 text-[hsl(var(--marketing-text-muted))]">
              <a
                href="https://twitter.com/TryMapleAI"
                target="_blank"
                rel="noopener noreferrer"
                className="dark:hover:text-[hsl(var(--blue))] hover:text-[hsl(var(--maple-primary))] transition-colors"
                aria-label="Twitter"
              >
                <Twitter className="h-5 w-5" />
              </a>
              <a
                href="https://github.com/OpenSecretCloud"
                target="_blank"
                rel="noopener noreferrer"
                className="dark:hover:text-[hsl(var(--blue))] hover:text-[hsl(var(--maple-primary))] transition-colors"
                aria-label="GitHub"
              >
                <Github className="h-5 w-5" />
              </a>
              <a
                href="https://discord.gg/ch2gjZAMGy"
                target="_blank"
                rel="noopener noreferrer"
                className="dark:hover:text-[hsl(var(--blue))] hover:text-[hsl(var(--maple-primary))] transition-colors"
                aria-label="Discord"
              >
                <DiscordIcon className="h-5 w-5" />
              </a>
              <a
                href="mailto:team@trymaple.ai"
                className="dark:hover:text-[hsl(var(--blue))] hover:text-[hsl(var(--green))] transition-colors"
                aria-label="Email"
              >
                <Mail className="h-5 w-5" />
              </a>
            </div>
          </div>

          <div className="flex flex-col gap-3">
            <h3 className="text-foreground text-sm font-medium tracking-wide uppercase">
              Products
            </h3>
            <Link to="/agent" className={footerLink}>
              Maple Agent
            </Link>
            <Link to="/research" className={footerLink}>
              Maple Research
            </Link>
            <a
              href="https://blog.trymaple.ai/maple-proxy-documentation/"
              target="_blank"
              rel="noopener noreferrer"
              className={footerLink}
            >
              Maple Proxy API
            </a>
            <Link to="/downloads" className={footerLink}>
              Downloads
            </Link>
            <Link to="/pricing" className={footerLink}>
              Pricing
            </Link>
          </div>

          <div className="flex flex-col gap-3">
            <h3 className="text-foreground text-sm font-medium tracking-wide uppercase">
              Solutions
            </h3>
            <Link to="/solutions/lawyers" className={footerLink}>
              AI for Lawyers
            </Link>
            <Link to="/solutions/accountants" className={footerLink}>
              AI for Accountants
            </Link>
            <Link to="/solutions/finance" className={footerLink}>
              AI for Finance
            </Link>
            <Link to="/solutions/therapy" className={footerLink}>
              AI for Therapy
            </Link>
            <Link to="/solutions/teams" className={footerLink}>
              AI for Teams
            </Link>
            <Link to="/solutions" className={footerLink}>
              All solutions
            </Link>
          </div>

          <div className="flex flex-col gap-3">
            <h3 className="text-foreground text-sm font-medium tracking-wide uppercase">
              Resources
            </h3>
            <a
              href="https://blog.trymaple.ai"
              target="_blank"
              rel="noopener noreferrer"
              className={footerLink}
            >
              Blog
            </a>
            <a
              href="https://status.trymaple.ai"
              target="_blank"
              rel="noopener noreferrer"
              className={footerLink}
            >
              Status
            </a>
            <Link to="/proof" className={footerLink}>
              Proof (Security &amp; Attestation)
            </Link>
            <a
              href="https://blog.trymaple.ai/tag/guides/"
              target="_blank"
              rel="noopener noreferrer"
              className={footerLink}
            >
              Support / Guides
            </a>
            <a href="mailto:team@trymaple.ai" className={footerLink}>
              Contact
            </a>
          </div>

          <div className="flex flex-col gap-3">
            <h3 className="text-foreground text-sm font-medium tracking-wide uppercase">Company</h3>
            <Link to="/about" className={footerLink}>
              About Us
            </Link>
            <a
              href="https://github.com/OpenSecretCloud/Maple"
              target="_blank"
              rel="noopener noreferrer"
              className={footerLink}
            >
              Maple Client
            </a>
            <a
              href="https://discord.gg/ch2gjZAMGy"
              target="_blank"
              rel="noopener noreferrer"
              className={footerLink}
            >
              Community (Discord)
            </a>
            <a
              href="https://github.com/OpenSecretCloud"
              target="_blank"
              rel="noopener noreferrer"
              className={footerLink}
            >
              Open Source (GitHub)
            </a>
            <a
              href="https://opensecret.cloud"
              target="_blank"
              rel="noopener noreferrer"
              className={footerLink}
            >
              OpenSecret Server
            </a>
            <a
              href="https://github.com/opensecretcloud/maple-proxy"
              target="_blank"
              rel="noopener noreferrer"
              className={footerLink}
            >
              Maple Proxy
            </a>
          </div>
        </div>

        <div className="mt-12 pt-8 border-t border-[hsl(var(--marketing-card-border))] flex flex-col lg:flex-row lg:items-center lg:justify-between gap-6">
          <div className="flex flex-wrap gap-x-6 gap-y-2 justify-center lg:justify-start text-sm">
            <a
              href="https://opensecret.cloud/privacy"
              target="_blank"
              rel="noopener noreferrer"
              className={footerLink}
            >
              Privacy Policy
            </a>
            <a
              href="https://opensecret.cloud/terms"
              target="_blank"
              rel="noopener noreferrer"
              className={footerLink}
            >
              Terms of Service
            </a>
            <Link to="/proof" className={footerLink}>
              Security Overview
            </Link>
          </div>
          {!isTauriPlatform && (
            <div className="flex justify-center lg:justify-end">
              <iframe
                src="https://status.trymaple.ai/badge?theme=system"
                width="250"
                height="30"
                frameBorder="0"
                scrolling="no"
                style={{ colorScheme: "normal", marginLeft: "58px" }}
                title="BetterStack Status"
              />
            </div>
          )}
        </div>

        <p className="mt-8 text-center text-[hsl(var(--marketing-text-muted))]/50 font-light text-sm">
          © {new Date().getFullYear()} Maple AI. All rights reserved. Powered by{" "}
          <a
            href="https://opensecret.cloud"
            target="_blank"
            rel="noopener noreferrer"
            className="text-[hsl(var(--maple-primary))] hover:text-[hsl(var(--maple-primary))]/80 transition-colors"
          >
            OpenSecret
          </a>
        </p>
      </div>
    </div>
  );
}
