import { isTauri } from "@/utils/platform";

export function SimplifiedFooter() {
  const isTauriPlatform = isTauri();

  return (
    <div className="w-full border-t border-[hsl(var(--marketing-card-border))] py-6 mt-auto">
      <div className="max-w-[45rem] mx-auto px-4 flex flex-col items-center">
        {/* TODO: Fix iframe loading in Tauri release builds */}
        {!isTauriPlatform && (
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
        )}
        <p className="text-[hsl(var(--marketing-text-muted))]/70 text-sm text-center">
          © {new Date().getFullYear()} Maple Privacy Labs Inc. All rights reserved.
        </p>
        <div className="flex justify-center gap-6 mt-2">
          <a
            href="/downloads"
            className="text-[hsl(var(--marketing-text-muted))] hover:text-foreground text-sm transition-colors"
          >
            Downloads
          </a>
          <a
            href="/terms"
            className="text-[hsl(var(--marketing-text-muted))] hover:text-foreground text-sm transition-colors"
          >
            Terms of Service
          </a>
          <a
            href="/privacy"
            className="text-[hsl(var(--marketing-text-muted))] hover:text-foreground text-sm transition-colors"
          >
            Privacy Policy
          </a>
        </div>
      </div>
    </div>
  );
}
