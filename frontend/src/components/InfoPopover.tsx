import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTrigger,
  DialogTitle
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Lock } from "lucide-react";
import { InfoContent } from "./Explainer";

export function InfoPopover() {
  return (
    <div className="fixed top-4 right-4 md:top-8 z-20">
      <Dialog>
        <DialogTrigger asChild>
          <Button variant="outline" size="icon" className="gap-2 p-1! opacity-50 hover:opacity-100">
            <Lock className="h-4 w-4 " />
          </Button>
        </DialogTrigger>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>Maple AI</DialogTitle>
          </DialogHeader>
          <DialogDescription>
            <div className="flex gap-4 flex-col items-center">
              <InfoContent />
            </div>
          </DialogDescription>
        </DialogContent>
      </Dialog>
    </div>
  );
}
