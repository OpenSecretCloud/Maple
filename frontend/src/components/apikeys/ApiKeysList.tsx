import { useState } from "react";
import { Button } from "@/components/ui/button";
import { Trash2, Calendar, Loader2 } from "lucide-react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle
} from "@/components/ui/alert-dialog";
import { useOpenSecret } from "@opensecret/react";

interface ApiKey {
  name: string;
  created_at: string;
}

interface ApiKeysListProps {
  apiKeys: ApiKey[];
  onKeyDeleted: () => void;
}

export function ApiKeysList({ apiKeys, onKeyDeleted }: ApiKeysListProps) {
  const [deleteKeyName, setDeleteKeyName] = useState<string | null>(null);
  const [isDeleting, setIsDeleting] = useState(false);
  const { deleteApiKey } = useOpenSecret();

  const handleDelete = async () => {
    if (!deleteKeyName) return;

    setIsDeleting(true);
    try {
      await deleteApiKey(deleteKeyName);
      console.log(`API key "${deleteKeyName}" deleted successfully`);
      onKeyDeleted();
    } catch (error) {
      console.error("Failed to delete API key:", error);
      console.error("Failed to delete API key. Please try again.");
    } finally {
      setIsDeleting(false);
      setDeleteKeyName(null);
    }
  };

  const formatDate = (dateString: string) => {
    const date = new Date(dateString);
    return new Intl.DateTimeFormat("en-US", {
      month: "short",
      day: "numeric",
      year: "numeric",
      hour: "2-digit",
      minute: "2-digit"
    }).format(date);
  };

  return (
    <>
      <div className="rounded-lg border border-muted">
        <div className="divide-y divide-border">
          {apiKeys.map((key) => (
            <div
              key={key.name}
              className="p-3 flex items-center justify-between gap-3 hover:bg-muted/30 transition-colors"
            >
              <div className="min-w-0 flex-1">
                <p className="font-medium text-sm truncate">{key.name}</p>
                <p className="text-xs text-muted-foreground flex items-center gap-1 mt-0.5">
                  <Calendar className="h-3 w-3" />
                  Created {formatDate(key.created_at)}
                </p>
              </div>
              <Button
                size="sm"
                variant="ghost"
                className="h-7 w-7 p-0"
                onClick={() => setDeleteKeyName(key.name)}
              >
                <Trash2 className="h-3.5 w-3.5" />
              </Button>
            </div>
          ))}
        </div>
      </div>

      {/* Delete confirmation dialog */}
      <AlertDialog open={!!deleteKeyName} onOpenChange={() => setDeleteKeyName(null)}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete API Key</AlertDialogTitle>
            <AlertDialogDescription>
              Are you sure you want to delete the API key "{deleteKeyName}"? This action cannot be
              undone and any applications using this key will stop working immediately.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={isDeleting}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              onClick={handleDelete}
              disabled={isDeleting}
              className="bg-destructive text-white hover:bg-destructive/90"
            >
              {isDeleting ? (
                <>
                  <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                  Deleting...
                </>
              ) : (
                "Delete"
              )}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </>
  );
}
