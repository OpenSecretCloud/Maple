import { useEffect, useState, useCallback, useRef } from "react";
import { CheckCircle, AlertCircle, X, Server } from "lucide-react";
import { cn } from "@/utils/utils";

export interface Notification {
  id: string;
  type: "success" | "error" | "info";
  title: string;
  message?: string;
  icon?: React.ReactNode;
  duration?: number; // ms, 0 = permanent
}

interface GlobalNotificationProps {
  notification: Notification | null;
  onDismiss: () => void;
}

export function GlobalNotification({ notification, onDismiss }: GlobalNotificationProps) {
  const [isVisible, setIsVisible] = useState(false);
  const [isLeaving, setIsLeaving] = useState(false);
  const dismissTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const handleDismiss = useCallback(() => {
    setIsLeaving(true);
    dismissTimeoutRef.current = setTimeout(() => {
      setIsVisible(false);
      onDismiss();
    }, 200); // Match animation duration
  }, [onDismiss]);

  useEffect(() => {
    if (notification) {
      // Trigger enter animation
      setIsVisible(true);
      setIsLeaving(false);

      // Auto-dismiss after duration
      if (notification.duration && notification.duration > 0) {
        const timer = setTimeout(() => {
          handleDismiss();
        }, notification.duration);
        return () => clearTimeout(timer);
      }
    } else {
      setIsVisible(false);
    }
  }, [notification, handleDismiss]);

  // Cleanup dismiss timeout on unmount
  useEffect(() => {
    return () => {
      if (dismissTimeoutRef.current) {
        clearTimeout(dismissTimeoutRef.current);
      }
    };
  }, []);

  if (!notification || !isVisible) return null;

  const getIcon = () => {
    if (notification.icon) return notification.icon;

    switch (notification.type) {
      case "success":
        return <CheckCircle className="h-5 w-5 text-green-600 dark:text-green-500" />;
      case "error":
        return <AlertCircle className="h-5 w-5 text-destructive" />;
      case "info":
        return <Server className="h-5 w-5 text-primary" />;
    }
  };

  return (
    <div className="fixed top-4 right-4 z-50 pointer-events-none">
      <div
        className={cn(
          "pointer-events-auto flex items-start gap-3 rounded-lg border bg-card text-card-foreground p-4 shadow-lg transition-all duration-200 min-w-[320px] max-w-md",
          isLeaving ? "opacity-0 translate-x-full" : "opacity-100 translate-x-0",
          notification.type === "error" && "border-destructive/50 dark:border-destructive",
          notification.type === "success" && "border-green-500/50 dark:border-green-500",
          notification.type === "info" && "border-primary/50"
        )}
      >
        <div className="flex-shrink-0">{getIcon()}</div>

        <div className="flex-1 min-w-0">
          <h4 className="text-sm font-medium leading-tight">{notification.title}</h4>
          {notification.message && (
            <p className="text-xs text-muted-foreground mt-1">{notification.message}</p>
          )}
        </div>

        <button
          onClick={handleDismiss}
          className="flex-shrink-0 rounded-sm opacity-70 hover:opacity-100 transition-opacity"
        >
          <X className="h-4 w-4" />
          <span className="sr-only">Close</span>
        </button>
      </div>
    </div>
  );
}
