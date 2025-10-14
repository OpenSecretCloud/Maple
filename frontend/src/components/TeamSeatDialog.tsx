import { useState } from "react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { AlertCircle, Info } from "lucide-react";

interface TeamSeatDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: (seats: number) => void;
}

export function TeamSeatDialog({ open, onOpenChange, onConfirm }: TeamSeatDialogProps) {
  const [seats, setSeats] = useState<string>("2");
  const [error, setError] = useState<string | null>(null);

  const handleConfirm = (e: React.FormEvent) => {
    e.preventDefault();

    const numSeats = parseInt(seats, 10);

    if (isNaN(numSeats)) {
      setError("Please enter a valid number");
      return;
    }

    if (numSeats < 2) {
      setError("Minimum 2 seats required");
      return;
    }

    if (numSeats > 100) {
      setError("Maximum 100 seats allowed");
      return;
    }

    onConfirm(numSeats);
    onOpenChange(false);
    setSeats("2");
    setError(null);
  };

  const handleChange = (value: string) => {
    setSeats(value);
    setError(null);
  };

  const handleOpenChange = (newOpen: boolean) => {
    onOpenChange(newOpen);
    if (!newOpen) {
      setSeats("2");
      setError(null);
    }
  };

  return (
    <Dialog open={open} onOpenChange={handleOpenChange}>
      <DialogContent className="sm:max-w-[425px]">
        <DialogHeader>
          <DialogTitle>Select Team Seats</DialogTitle>
          <DialogDescription>
            How many seats would you like to purchase? You can change this at any time.
          </DialogDescription>
        </DialogHeader>

        <form onSubmit={handleConfirm}>
          <div className="grid gap-4 py-4">
            <div className="grid gap-2">
              <Label htmlFor="seats">Number of Seats</Label>
              <Input
                id="seats"
                type="number"
                min="2"
                max="100"
                value={seats}
                onChange={(e) => handleChange(e.target.value)}
                className="w-full"
                autoFocus
              />
              <p className="text-sm text-muted-foreground">Minimum 2 seats, maximum 100 seats</p>
            </div>

            <Alert>
              <Info className="h-4 w-4" />
              <AlertDescription>
                Each seat is billed per user per month. You can adjust the number of seats anytime
                in your billing settings.
              </AlertDescription>
            </Alert>

            {error && (
              <Alert variant="destructive">
                <AlertCircle className="h-4 w-4" />
                <AlertDescription>{error}</AlertDescription>
              </Alert>
            )}
          </div>

          <DialogFooter>
            <Button type="button" variant="outline" onClick={() => handleOpenChange(false)}>
              Cancel
            </Button>
            <Button type="submit">Continue to Checkout</Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
