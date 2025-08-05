export function SimplifiedFooter() {
  return (
    <div className="w-full border-t border-[hsl(var(--marketing-card-border))] py-6 mt-auto">
      <div className="max-w-[45rem] mx-auto px-4 flex flex-col items-center">
        <iframe
          src="https://status.trymaple.ai/badge?theme=system"
          width="250"
          height="30"
          frameBorder="0"
          scrolling="no"
          style={{ colorScheme: "normal", marginLeft: "58px" }}
          title="BetterStack Status"
          className="mb-3"
        />
        <p className="text-[hsl(var(--marketing-text-muted))]/70 text-sm text-center">
          © {new Date().getFullYear()} Maple AI. All rights reserved. Powered by{" "}
          <a
            href="https://opensecret.cloud"
            target="_blank"
            rel="noopener noreferrer"
            className="text-[hsl(var(--purple))] hover:text-[hsl(var(--purple))]/80 dark:text-[hsl(var(--blue))] dark:hover:text-[hsl(var(--blue))]/80 transition-colors"
          >
            OpenSecret
          </a>
        </p>
        <div className="flex justify-center gap-6 mt-2">
          <a
            href="/downloads"
            className="text-[hsl(var(--marketing-text-muted))] hover:text-foreground text-sm transition-colors"
          >
            Downloads
          </a>
          <a
            href="https://opensecret.cloud/terms"
            target="_blank"
            rel="noopener noreferrer"
            className="text-[hsl(var(--marketing-text-muted))] hover:text-foreground text-sm transition-colors"
          >
            Terms of Service
          </a>
          <a
            href="https://opensecret.cloud/privacy"
            target="_blank"
            rel="noopener noreferrer"
            className="text-[hsl(var(--marketing-text-muted))] hover:text-foreground text-sm transition-colors"
          >
            Privacy Policy
          </a>
        </div>
      </div>
    </div>
  );
}
