import React, { createContext, useContext, useState, useCallback, useMemo } from "react";
import { GlobalNotification, type Notification } from "@/components/GlobalNotification";

interface NotificationContextType {
  showNotification: (notification: Omit<Notification, "id">) => void;
  clearNotification: () => void;
}

const NotificationContext = createContext<NotificationContextType | undefined>(undefined);

export function NotificationProvider({ children }: { children: React.ReactNode }) {
  const [notification, setNotification] = useState<Notification | null>(null);

  const showNotification = useCallback((notif: Omit<Notification, "id">) => {
    const id = Date.now().toString();
    setNotification({
      ...notif,
      id,
      duration: notif.duration ?? 5000 // Default 5 seconds
    });
  }, []);

  const clearNotification = useCallback(() => {
    setNotification(null);
  }, []);

  const contextValue = useMemo(
    () => ({ showNotification, clearNotification }),
    [clearNotification, showNotification]
  );

  return (
    <NotificationContext.Provider value={contextValue}>
      {children}
      <GlobalNotification notification={notification} onDismiss={clearNotification} />
    </NotificationContext.Provider>
  );
}

export function useNotification() {
  const context = useContext(NotificationContext);
  if (!context) {
    throw new Error("useNotification must be used within NotificationProvider");
  }
  return context;
}
