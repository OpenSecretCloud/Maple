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

interface DeleteConversationProjectDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void;
  projectName: string;
  conversationCount?: number;
}

export function DeleteConversationProjectDialog({
  open,
  onOpenChange,
  onConfirm,
  projectName,
  conversationCount = 0
}: DeleteConversationProjectDialogProps) {
  function handleConfirm(event: React.MouseEvent) {
    event.preventDefault();
    onConfirm();
    onOpenChange(false);
  }

  return (
    <AlertDialog open={open} onOpenChange={onOpenChange}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Delete this project?</AlertDialogTitle>
          <AlertDialogDescription>
            {conversationCount > 0
              ? `This will permanently delete "${projectName}" and its ${conversationCount} chat${
                  conversationCount === 1 ? "" : "s"
                }. This action cannot be undone.`
              : `This will permanently delete "${projectName}". This action cannot be undone.`}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction
            onClick={handleConfirm}
            className="bg-destructive text-white hover:bg-destructive/90"
          >
            Delete Project
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
