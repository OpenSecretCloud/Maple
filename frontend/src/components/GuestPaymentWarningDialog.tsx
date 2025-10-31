import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle
} from "@/components/ui/dialog";
import { useNavigate } from "@tanstack/react-router";
import { useOpenSecret } from "@opensecret/react";
import { AlertTriangle, LogOut, CreditCard } from "lucide-react";

interface GuestPaymentWarningDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export function GuestPaymentWarningDialog({ open, onOpenChange }: GuestPaymentWarningDialogProps) {
  const navigate = useNavigate();
  const os = useOpenSecret();

  const handleGoToPricing = () => {
    navigate({ to: "/pricing" });
  };

  const handleLogout = async () => {
    await os.signOut();
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent
        className="sm:max-w-[425px] [&>button]:hidden"
        onInteractOutside={(e) => e.preventDefault()}
        onEscapeKeyDown={(e) => e.preventDefault()}
      >
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2 text-amber-600 dark:text-amber-500">
            <AlertTriangle className="w-5 h-5" />
            Subscription Required
          </DialogTitle>
          <DialogDescription>
            Anonymous accounts require a paid subscription to use Maple AI.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          <div className="rounded-lg border border-amber-500/50 bg-amber-500/10 p-4 space-y-3">
            <p className="text-sm font-medium">
              Your anonymous account is not activated yet and cannot use the chat feature.
            </p>
            <p className="text-sm text-muted-foreground">
              To start chatting with Maple AI, you need to subscribe to a paid plan. Anonymous
              accounts must pay for a full year using Bitcoin.
            </p>
          </div>

          <div className="space-y-2">
            <Button onClick={handleGoToPricing} className="w-full gap-2">
              <CreditCard className="w-4 h-4" />
              View Pricing & Subscribe
            </Button>
            <Button variant="outline" onClick={handleLogout} className="w-full gap-2">
              <LogOut className="w-4 h-4" />
              Log Out
            </Button>
          </div>
        </div>
      </DialogContent>
    </Dialog>
  );
}
