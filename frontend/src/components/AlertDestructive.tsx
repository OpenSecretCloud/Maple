import { AlertCircle } from "lucide-react";

import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";

export function AlertDestructive({ title, description }: { title?: string; description?: string }) {
  return (
    <Alert variant="destructive" className="bg-background">
      <AlertCircle className="h-4 w-4" />
      <AlertTitle>{title ?? "Error"}</AlertTitle>
      <AlertDescription>
        {description ?? "Something went wrong. Please try again."}
      </AlertDescription>
    </Alert>
  );
}
