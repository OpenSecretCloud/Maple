import { Link } from "@tanstack/react-router";
import { useEffect, useState } from "react";
import { Loader2, CheckCircle, XCircle } from "lucide-react";
import { useOpenSecret } from "@opensecret/react";

function VerificationStatus() {
  const [status, setStatus] = useState<"loading" | "verified" | "failed">("loading");
  const os = useOpenSecret();

  useEffect(() => {
    async function verify() {
      try {
        const verified = await os.getAttestation();
        await new Promise((resolve) => setTimeout(resolve, 800));
        setStatus(verified ? "verified" : "failed");
      } catch (error) {
        console.error("Verification failed:", error);
        setStatus("failed");
      }
    }
    verify();
  }, [os]);

  return (
    <Link
      to="/proof"
      className="flex items-center gap-1 text-sm font-medium hover:underline"
      target="_blank"
    >
      {status === "loading" && (
        <>
          <Loader2 className="h-3 w-3 animate-spin" />
          <span className="text-muted-foreground">Verifying...</span>
        </>
      )}
      {status === "verified" && (
        <>
          <CheckCircle className="h-3 w-3 text-green-700 dark:text-green-500" />
          <span className="text-green-700 dark:text-green-500">Verified</span>
        </>
      )}
      {status === "failed" && (
        <>
          <XCircle className="h-3 w-3 text-red-700 dark:text-red-500" />
          <span className="text-red-700 dark:text-red-500">Verification failed</span>
        </>
      )}
    </Link>
  );
}

export function SimplifiedFooter() {
  return (
    <div className="w-full border-t border-[hsl(var(--marketing-card-border))] py-6 mt-auto">
      <div className="max-w-[45rem] mx-auto px-4 flex flex-col items-center">
        <div className="flex items-center gap-6 mb-3">
          <iframe
            src="https://status.trymaple.ai/badge?theme=system"
            width="250"
            height="30"
            frameBorder="0"
            scrolling="no"
            style={{ colorScheme: "normal" }}
            title="BetterStack Status"
          />
          <VerificationStatus />
        </div>
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
