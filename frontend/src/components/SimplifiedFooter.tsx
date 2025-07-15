export function SimplifiedFooter() {
  const handleExternalLink = async (url: string) => {
    try {
      // Use Tauri opener plugin to open external URLs in the device's default browser
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("plugin:opener|open_url", { url });
    } catch (error) {
      // Fallback for non-Tauri environments (e.g., web)
      console.warn("Failed to open URL with Tauri opener, falling back to window.open:", error);
      window.open(url, "_blank", "noopener,noreferrer");
    }
  };

  return (
    <div className="w-full border-t border-[hsl(var(--marketing-card-border))] py-6 mt-auto">
      <div className="max-w-[45rem] mx-auto px-4 text-center">
        <p className="text-[hsl(var(--marketing-text-muted))]/70 text-sm">
          © {new Date().getFullYear()} Maple AI. All rights reserved. Powered by{" "}
          <button
            onClick={() => handleExternalLink("https://opensecret.cloud")}
            className="text-[hsl(var(--purple))] hover:text-[hsl(var(--purple))]/80 dark:text-[hsl(var(--blue))] dark:hover:text-[hsl(var(--blue))]/80 transition-colors underline"
          >
            OpenSecret
          </button>
        </p>
        <div className="flex justify-center gap-6 mt-2">
          <a
            href="/downloads"
            className="text-[hsl(var(--marketing-text-muted))] hover:text-foreground text-sm transition-colors"
          >
            Downloads
          </a>
          <button
            onClick={() => handleExternalLink("https://opensecret.cloud/terms")}
            className="text-[hsl(var(--marketing-text-muted))] hover:text-foreground text-sm transition-colors"
          >
            Terms of Service
          </button>
          <button
            onClick={() => handleExternalLink("https://opensecret.cloud/privacy")}
            className="text-[hsl(var(--marketing-text-muted))] hover:text-foreground text-sm transition-colors"
          >
            Privacy Policy
          </button>
        </div>
      </div>
    </div>
  );
}
