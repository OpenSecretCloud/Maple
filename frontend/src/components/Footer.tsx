import { Github, Twitter, Mail } from "lucide-react";
import { DiscordIcon } from "./icons/DiscordIcon";

export function Footer() {
  return (
    <div className="text-center">
      <h3 className="text-white text-2xl pt-4 font-light">
        Powered by{" "}
        <a
          href="https://opensecret.cloud"
          target="_blank"
          rel="noopener noreferrer"
          className="underline"
        >
          OpenSecret
        </a>
      </h3>
      <div className="mt-2 text-sm text-white/70">
        <a
          href="https://opensecret.cloud/terms"
          target="_blank"
          rel="noopener noreferrer"
          className="hover:text-white/90"
        >
          Terms of Service
        </a>
        {" | "}
        <a
          href="https://opensecret.cloud/privacy"
          target="_blank"
          rel="noopener noreferrer"
          className="hover:text-white/90"
        >
          Privacy Policy
        </a>
      </div>
      <div className="mt-4 flex justify-center gap-6 text-white/70">
        <a
          href="https://twitter.com/try_maple_ai"
          target="_blank"
          rel="noopener noreferrer"
          className="hover:text-white/90"
          aria-label="Twitter"
        >
          <Twitter className="h-5 w-5" />
        </a>
        <a
          href="https://github.com/OpenSecretCloud"
          target="_blank"
          rel="noopener noreferrer"
          className="hover:text-white/90"
          aria-label="GitHub"
        >
          <Github className="h-5 w-5" />
        </a>
        <a
          href="https://discord.gg/ch2gjZAMGy"
          target="_blank"
          rel="noopener noreferrer"
          className="hover:text-white/90"
          aria-label="Discord"
        >
          <DiscordIcon className="h-5 w-5" />
        </a>
        <a href="mailto:team@opensecret.cloud" className="hover:text-white/90" aria-label="Email">
          <Mail className="h-5 w-5" />
        </a>
      </div>
    </div>
  );
}
