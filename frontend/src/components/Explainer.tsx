import { Link } from "@tanstack/react-router";
import { ArrowRight, BotIcon, LockIcon, MinusIcon, ServerIcon, SmartphoneIcon } from "lucide-react";
import { useEffect, useState } from "react";
import { Loader2, CheckCircle, XCircle } from "lucide-react";
import { useOpenSecret } from "@opensecret/react";

function ArrowAndLock() {
  return (
    <>
      <div className="flex pt-2 -mx-2 max-sm:hidden">
        <MinusIcon className="h-4 w-4 text-muted-foreground" />
        <LockIcon className="h-4 w-4 text-muted-foreground" />
        <ArrowRight className="h-4 w-4 text-muted-foreground" />
      </div>
      {/* only visible on mobile */}
      <div className="flex flex-col py-2 sm:hidden ">
        <MinusIcon className="h-4 w-4 text-muted-foreground rotate-90" />
        <LockIcon className="h-4 w-4 text-muted-foreground" />
        <ArrowRight className="h-4 w-4 text-muted-foreground rotate-90" />
      </div>
    </>
  );
}

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
    <Link to="/proof" className="flex items-center gap-1 text-sm font-medium hover:underline">
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

export function InfoContent() {
  return (
    <>
      <p className="text-center">
        Encrypted. At every step.
        <br />
        Nobody can read your chats but you.
      </p>
      <div className="flex flex-col items-center justify-center pt-4 sm:flex-row sm:items-start">
        <div className="flex flex-col gap-2 items-center w-[6rem]">
          <SmartphoneIcon className="h-8 w-8 text-primary" />
          <span className="text-center text-sm text-muted-foreground">Your device</span>
        </div>
        <ArrowAndLock />
        <div className="flex flex-col gap-2 items-center w-[6rem]">
          <ServerIcon className="h-8 w-8 text-primary" />
          <span className="text-center text-sm text-muted-foreground">Secure server</span>
        </div>
        <ArrowAndLock />
        <div className="flex flex-col gap-2 items-center w-[6rem]">
          <BotIcon className="h-8 w-8 text-primary" />
          <span className="text-center text-sm text-muted-foreground">AI cloud</span>
        </div>
      </div>
      <div className="w-full pt-4 flex gap-4 items-center justify-between">
        <VerificationStatus />
        <Link to="/about" className="text-center hover:underline font-medium text-sm">
          Learn more
        </Link>
      </div>
    </>
  );
}
