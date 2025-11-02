import { useEffect, useState } from "react";
import { ExternalUrlConfirmDialog } from "@/components/ExternalUrlConfirmDialog";
import { setUrlConfirmationCallback, openExternalUrl } from "@/utils/openUrl";

export function ExternalUrlConfirmHandler() {
  const [pendingUrl, setPendingUrl] = useState<string | null>(null);

  useEffect(() => {
    const register = (url: string) => {
      // Only set if no pending URL (ignore rapid clicks while dialog is open)
      setPendingUrl((current) => (current !== null ? current : url));
    };
    setUrlConfirmationCallback(register);
    return () => setUrlConfirmationCallback(null);
  }, []);

  const handleConfirm = () => {
    if (pendingUrl) {
      openExternalUrl(pendingUrl);
      setPendingUrl(null);
    }
  };

  return (
    <ExternalUrlConfirmDialog
      open={pendingUrl !== null}
      onOpenChange={(open) => !open && setPendingUrl(null)}
      onConfirm={handleConfirm}
      url={pendingUrl || ""}
    />
  );
}
