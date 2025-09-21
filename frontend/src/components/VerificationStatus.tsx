import { useOpenSecret } from "@opensecret/react";
import { Link } from "@tanstack/react-router";
import { CheckCircle, Loader2, XCircle } from "lucide-react";
import { useEffect, useState } from "react";

export function VerificationStatus() {
  const [status, setStatus] = useState<"loading" | "verified" | "failed">("loading");
  const os = useOpenSecret();

  useEffect(() => {
    let cancelled = false;
    async function verify() {
      try {
        const verified = await os.getAttestation();
        await new Promise((resolve) => setTimeout(resolve, 800));
        if (cancelled) return;
        setStatus(verified ? "verified" : "failed");
      } catch (error) {
        if (cancelled) return;
        console.error("Verification failed:", error);
        setStatus("failed");
      }
    }
    verify();
    return () => {
      cancelled = true;
    };
  }, [os]);

  return (
    <Link
      to="/proof"
      className="flex items-center gap-1 text-sm font-medium hover:underline"
      target="_blank"
      rel="noopener noreferrer"
      aria-live="polite"
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
