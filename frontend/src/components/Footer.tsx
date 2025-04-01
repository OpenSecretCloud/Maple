import { Github, Twitter, Mail } from "lucide-react";
import { DiscordIcon } from "./icons/DiscordIcon";

export function Footer() {
  return (
    <div className="w-full dark:bg-[hsl(var(--background))] bg-[hsl(var(--footer-bg))] py-16 border-t border-[hsl(var(--marketing-card-border))]">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
        <div className="grid grid-cols-1 md:grid-cols-4 gap-8">
          <div className="flex flex-col gap-4">
            <img src="/maple-logo-dark.svg" alt="Maple" className="w-32 block dark:hidden" />
            <img src="/maple-logo.svg" alt="Maple" className="w-32 hidden dark:block" />
            <p className="text-[hsl(var(--marketing-text-muted))] font-light">
              Private AI chat with end-to-end encryption. Your conversations stay yours.
            </p>
            <div className="flex gap-5 text-[hsl(var(--marketing-text-muted))]">
              <a
                href="https://twitter.com/TryMapleAI"
                target="_blank"
                rel="noopener noreferrer"
                className="dark:hover:text-[hsl(var(--blue))] hover:text-[hsl(var(--purple))] transition-colors"
                aria-label="Twitter"
              >
                <Twitter className="h-5 w-5" />
              </a>
              <a
                href="https://github.com/OpenSecretCloud"
                target="_blank"
                rel="noopener noreferrer"
                className="dark:hover:text-[hsl(var(--blue))] hover:text-[hsl(var(--purple))] transition-colors"
                aria-label="GitHub"
              >
                <Github className="h-5 w-5" />
              </a>
              <a
                href="https://discord.gg/ch2gjZAMGy"
                target="_blank"
                rel="noopener noreferrer"
                className="dark:hover:text-[hsl(var(--blue))] hover:text-[hsl(var(--purple))] transition-colors"
                aria-label="Discord"
              >
                <DiscordIcon className="h-5 w-5" />
              </a>
              <a
                href="mailto:team@opensecret.cloud"
                className="dark:hover:text-[hsl(var(--blue))] hover:text-[hsl(var(--green))] transition-colors"
                aria-label="Email"
              >
                <Mail className="h-5 w-5" />
              </a>
            </div>
          </div>

          <div className="flex flex-col gap-4">
            <h3 className="text-foreground text-lg font-medium">Product</h3>
            <a
              href="/pricing"
              className="text-[hsl(var(--marketing-text-muted))] hover:text-foreground transition-colors"
            >
              Pricing
            </a>
            <a
              href="/proof"
              className="text-[hsl(var(--marketing-text-muted))] hover:text-foreground transition-colors"
            >
              Security Proof
            </a>
          </div>

          <div className="flex flex-col gap-4">
            <h3 className="text-foreground text-lg font-medium">Resources</h3>
            <a
              href="https://blog.trymaple.ai"
              target="_blank"
              rel="noopener noreferrer"
              className="text-[hsl(var(--marketing-text-muted))] hover:text-foreground transition-colors"
            >
              Blog
            </a>
            <a
              href="https://opensecret.cloud"
              target="_blank"
              rel="noopener noreferrer"
              className="text-[hsl(var(--marketing-text-muted))] hover:text-foreground transition-colors"
            >
              OpenSecret
            </a>
            <a
              href="https://discord.gg/ch2gjZAMGy"
              target="_blank"
              rel="noopener noreferrer"
              className="text-[hsl(var(--marketing-text-muted))] hover:text-foreground transition-colors"
            >
              Community
            </a>
          </div>

          <div className="flex flex-col gap-4">
            <h3 className="text-foreground text-lg font-medium">Legal</h3>
            <a
              href="https://opensecret.cloud/terms"
              target="_blank"
              rel="noopener noreferrer"
              className="text-[hsl(var(--marketing-text-muted))] hover:text-foreground transition-colors"
            >
              Terms of Service
            </a>
            <a
              href="https://opensecret.cloud/privacy"
              target="_blank"
              rel="noopener noreferrer"
              className="text-[hsl(var(--marketing-text-muted))] hover:text-foreground transition-colors"
            >
              Privacy Policy
            </a>
          </div>
        </div>

        <div className="mt-12 pt-8 border-t border-[hsl(var(--marketing-card-border))] text-center">
          <p className="text-[hsl(var(--marketing-text-muted))]/50 font-light">
            © {new Date().getFullYear()} Maple AI. All rights reserved. Powered by{" "}
            <a
              href="https://opensecret.cloud"
              target="_blank"
              rel="noopener noreferrer"
              className="text-[hsl(var(--purple))] hover:text-[hsl(var(--purple))]/80 transition-colors"
            >
              OpenSecret
            </a>
          </p>
        </div>
      </div>
    </div>
  );
}
