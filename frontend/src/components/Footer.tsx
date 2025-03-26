import { Github, Twitter, Mail } from "lucide-react";
import { DiscordIcon } from "./icons/DiscordIcon";

export function Footer() {
  return (
    <div className="w-full bg-[#111111] py-16 border-t border-[#E2E2E2]/10">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
        <div className="grid grid-cols-1 md:grid-cols-4 gap-8">
          <div className="flex flex-col gap-4">
            <img src="/maple-logo.svg" alt="Maple" className="w-32" />
            <p className="text-[#E2E2E2]/70 font-light">
              Private AI chat with end-to-end encryption. Your conversations stay yours.
            </p>
            <div className="flex gap-5 text-[#E2E2E2]/70">
              <a
                href="https://twitter.com/TryMapleAI"
                target="_blank"
                rel="noopener noreferrer"
                className="hover:text-[#3FDBFF] transition-colors"
                aria-label="Twitter"
              >
                <Twitter className="h-5 w-5" />
              </a>
              <a
                href="https://github.com/OpenSecretCloud"
                target="_blank"
                rel="noopener noreferrer"
                className="hover:text-[#3FDBFF] transition-colors"
                aria-label="GitHub"
              >
                <Github className="h-5 w-5" />
              </a>
              <a
                href="https://discord.gg/ch2gjZAMGy"
                target="_blank"
                rel="noopener noreferrer"
                className="hover:text-[#3FDBFF] transition-colors"
                aria-label="Discord"
              >
                <DiscordIcon className="h-5 w-5" />
              </a>
              <a
                href="mailto:team@opensecret.cloud"
                className="hover:text-[#3FDBFF] transition-colors"
                aria-label="Email"
              >
                <Mail className="h-5 w-5" />
              </a>
            </div>
          </div>

          <div className="flex flex-col gap-4">
            <h3 className="text-[#E2E2E2] text-lg font-medium">Product</h3>
            <a href="/pricing" className="text-[#E2E2E2]/70 hover:text-[#E2E2E2] transition-colors">
              Pricing
            </a>
            <a href="/proof" className="text-[#E2E2E2]/70 hover:text-[#E2E2E2] transition-colors">
              Security Proof
            </a>
          </div>

          <div className="flex flex-col gap-4">
            <h3 className="text-[#E2E2E2] text-lg font-medium">Resources</h3>
            <a
              href="https://blog.opensecret.cloud"
              target="_blank"
              rel="noopener noreferrer"
              className="text-[#E2E2E2]/70 hover:text-[#E2E2E2] transition-colors"
            >
              Blog
            </a>
            <a
              href="https://opensecret.cloud"
              target="_blank"
              rel="noopener noreferrer"
              className="text-[#E2E2E2]/70 hover:text-[#E2E2E2] transition-colors"
            >
              OpenSecret
            </a>
            <a
              href="https://discord.gg/ch2gjZAMGy"
              target="_blank"
              rel="noopener noreferrer"
              className="text-[#E2E2E2]/70 hover:text-[#E2E2E2] transition-colors"
            >
              Community
            </a>
          </div>

          <div className="flex flex-col gap-4">
            <h3 className="text-[#E2E2E2] text-lg font-medium">Legal</h3>
            <a
              href="https://opensecret.cloud/terms"
              target="_blank"
              rel="noopener noreferrer"
              className="text-[#E2E2E2]/70 hover:text-[#E2E2E2] transition-colors"
            >
              Terms of Service
            </a>
            <a
              href="https://opensecret.cloud/privacy"
              target="_blank"
              rel="noopener noreferrer"
              className="text-[#E2E2E2]/70 hover:text-[#E2E2E2] transition-colors"
            >
              Privacy Policy
            </a>
          </div>
        </div>

        <div className="mt-12 pt-8 border-t border-[#E2E2E2]/10 text-center">
          <p className="text-[#E2E2E2]/50 font-light">
            © {new Date().getFullYear()} Maple AI. All rights reserved. Powered by{" "}
            <a
              href="https://opensecret.cloud"
              target="_blank"
              rel="noopener noreferrer"
              className="text-[#9469F8] hover:text-[#A57FF9] transition-colors"
            >
              OpenSecret
            </a>
          </p>
        </div>
      </div>
    </div>
  );
}
