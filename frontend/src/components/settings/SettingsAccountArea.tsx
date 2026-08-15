import { LogOut } from "lucide-react";
import type { ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { cn } from "@/utils/utils";

type SettingsAccountAreaProps = {
  compact: boolean;
  email: string;
  planLabel: string;
  signOutError: string | null;
  isSigningOut: boolean;
  signOutDisabled: boolean;
  onSignOut: () => void | Promise<void>;
  usage: ReactNode;
};

export const SETTINGS_USAGE_LINK_CLASS =
  "group/credit-link mt-2 flex min-h-11 min-w-0 rounded-xl outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background";

export function SettingsAccountArea({
  compact,
  email,
  planLabel,
  signOutError,
  isSigningOut,
  signOutDisabled,
  onSignOut,
  usage
}: SettingsAccountAreaProps) {
  return (
    <div
      className={cn(
        "shrink-0 border-t border-border/30",
        compact
          ? "maple-mobile-settings-account-footer px-5 pb-[max(1rem,env(safe-area-inset-bottom))] pt-2"
          : "p-3"
      )}
    >
      <div className={cn("mb-2 min-w-0", !compact && "px-3")}>
        <p className={cn("truncate font-medium", compact ? "text-base leading-6" : "text-xs")}>
          {email}
        </p>
        <p
          className={cn(
            "truncate text-muted-foreground",
            compact ? "text-xs leading-5" : "text-[11px]"
          )}
        >
          {planLabel}
        </p>
      </div>
      {signOutError && (
        <p
          role="alert"
          className={cn(
            "mb-2 leading-relaxed text-destructive",
            compact ? "text-sm" : "px-3 text-xs"
          )}
        >
          {signOutError}
        </p>
      )}
      <Button
        type="button"
        variant="ghost"
        onClick={() => void onSignOut()}
        disabled={signOutDisabled}
        title="Log out"
        className={cn(
          "w-full justify-start text-muted-foreground hover:text-foreground",
          compact ? "h-11 px-0 text-base" : "h-10 px-3"
        )}
      >
        <LogOut className={cn("shrink-0", compact ? "mr-2 h-5 w-5" : "mr-3 h-4 w-4")} />
        <span>{isSigningOut ? "Logging out..." : "Log out"}</span>
      </Button>
      {usage}
    </div>
  );
}
