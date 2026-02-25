import { Link } from "@tanstack/react-router";
import { Info, Shield, FileText, Mail, ExternalLink } from "lucide-react";
import { isTauri, isMobile } from "@/utils/platform";
import packageJson from "../../../package.json";

export function AboutSection() {
  const handleOpenExternalUrl = async (url: string) => {
    try {
      const isInTauri = isTauri();
      if (isInTauri) {
        const { invoke } = await import("@tauri-apps/api/core");
        await invoke("plugin:opener|open_url", { url })
          .then(() => console.log("[External Link] Opened URL"))
          .catch((err: Error) => {
            console.error("[External Link] Failed to open:", err);
            if (isMobile()) {
              alert("Failed to open link. Please try again.");
            } else {
              window.open(url, "_blank", "noopener,noreferrer");
            }
          });
      } else {
        window.open(url, "_blank", "noopener,noreferrer");
      }
    } catch (error) {
      console.error("Error opening external URL:", error);
      window.open(url, "_blank", "noopener,noreferrer");
    }
  };

  return (
    <div className="space-y-8">
      <div>
        <h2 className="text-2xl font-semibold tracking-tight">About</h2>
        <p className="text-muted-foreground mt-1">Learn more about Maple AI.</p>
      </div>

      {/* App Info */}
      <div className="space-y-4">
        <div className="flex items-center gap-3">
          <img
            src="/maple-app-icon-vector.svg"
            alt="Maple App Icon"
            className="w-12 h-12 rounded-lg"
          />
          <div>
            <h3 className="text-lg font-medium">Maple AI</h3>
            <p className="text-sm text-muted-foreground">Version {packageJson.version}</p>
          </div>
        </div>
        <p className="text-sm text-muted-foreground">
          Incredibly powerful AI that doesn't share your data with anyone. Built on OpenSecret with
          end-to-end encryption and confidential computing.
        </p>
      </div>

      {/* Links */}
      <div className="space-y-2">
        <h3 className="text-lg font-medium">Resources</h3>
        <div className="divide-y divide-input rounded-lg border border-input overflow-hidden">
          <Link
            to="/about"
            className="flex items-center gap-3 px-4 py-3 hover:bg-muted/50 transition-colors"
          >
            <Info className="h-4 w-4 text-muted-foreground flex-shrink-0" />
            <span className="flex-1 text-sm font-medium">About Maple</span>
            <ExternalLink className="h-4 w-4 text-muted-foreground" />
          </Link>

          <button
            onClick={() => handleOpenExternalUrl("https://opensecret.cloud/privacy")}
            className="flex items-center gap-3 px-4 py-3 hover:bg-muted/50 transition-colors w-full text-left"
          >
            <Shield className="h-4 w-4 text-muted-foreground flex-shrink-0" />
            <span className="flex-1 text-sm font-medium">Privacy Policy</span>
            <ExternalLink className="h-4 w-4 text-muted-foreground" />
          </button>

          <button
            onClick={() => handleOpenExternalUrl("https://opensecret.cloud/terms")}
            className="flex items-center gap-3 px-4 py-3 hover:bg-muted/50 transition-colors w-full text-left"
          >
            <FileText className="h-4 w-4 text-muted-foreground flex-shrink-0" />
            <span className="flex-1 text-sm font-medium">Terms of Service</span>
            <ExternalLink className="h-4 w-4 text-muted-foreground" />
          </button>

          <button
            onClick={() => handleOpenExternalUrl("mailto:support@opensecret.cloud")}
            className="flex items-center gap-3 px-4 py-3 hover:bg-muted/50 transition-colors w-full text-left"
          >
            <Mail className="h-4 w-4 text-muted-foreground flex-shrink-0" />
            <span className="flex-1 text-sm font-medium">Contact Us</span>
            <ExternalLink className="h-4 w-4 text-muted-foreground" />
          </button>
        </div>
      </div>

      {/* Built by */}
      <div className="pt-4 text-center">
        <p className="text-sm text-muted-foreground">
          Built by{" "}
          <button
            onClick={() => handleOpenExternalUrl("https://opensecret.cloud")}
            className="text-primary hover:underline"
          >
            OpenSecret
          </button>
        </p>
        <p className="text-xs text-muted-foreground mt-1">
          Built in Austin. Living in Secure Enclaves.
        </p>
      </div>
    </div>
  );
}
