import { Dialog, DialogContent } from "@/components/ui/dialog";
import { ApiKeyDashboard } from "./ApiKeyDashboard";

interface ApiKeyManagementDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  showCreditSuccessMessage?: boolean;
}

export function ApiKeyManagementDialog({
  open,
  onOpenChange,
  showCreditSuccessMessage = false
}: ApiKeyManagementDialogProps) {
  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg max-w-[calc(100vw-2rem)] max-h-[90vh] overflow-y-auto">
        <ApiKeyDashboard showCreditSuccessMessage={showCreditSuccessMessage} />
      </DialogContent>
    </Dialog>
  );
}
