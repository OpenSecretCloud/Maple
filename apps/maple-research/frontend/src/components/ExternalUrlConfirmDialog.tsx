import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";

interface ExternalUrlConfirmDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void;
  url: string;
}

export function ExternalUrlConfirmDialog({
  open,
  onOpenChange,
  onConfirm,
  url
}: ExternalUrlConfirmDialogProps) {
  const handleConfirm = (e: React.MouseEvent) => {
    e.preventDefault();
    onConfirm();
    onOpenChange(false);
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Open External Link?</DialogTitle>
          <DialogDescription>
            You are about to open an external URL in your browser:
            <div className="mt-2 p-2 bg-muted rounded-md break-all text-sm font-mono">{url}</div>
          </DialogDescription>
        </DialogHeader>
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Cancel
          </Button>
          <Button onClick={handleConfirm}>Open</Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
