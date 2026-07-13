import type { ReactNode, Ref } from "react";
import { SquarePen } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/utils/utils";

type ChatTurnProps = {
  children: ReactNode;
  actions?: ReactNode;
  containerRef?: Ref<HTMLDivElement>;
  className?: string;
};

export function MapleChatAvatar() {
  return (
    <img
      src="/m-avatar.svg"
      alt=""
      width={32}
      height={32}
      draggable={false}
      className="h-8 w-8 shrink-0 select-none"
    />
  );
}

export function ChatUserTurn({
  children,
  actions,
  containerRef,
  className,
  actionsClassName
}: ChatTurnProps & { actionsClassName?: string }) {
  return (
    <div
      ref={containerRef}
      className={cn("group/user flex flex-col items-end py-4 landscape-short:py-1.5", className)}
    >
      <div className="max-w-[min(100%,42rem)] rounded-2xl border border-border bg-muted px-4 py-3 backdrop-blur-lg dark:bg-card landscape-short:px-3 landscape-short:py-2">
        <div className="prose prose-sm max-w-none text-left dark:prose-invert">
          <div className="space-y-3">{children}</div>
        </div>
      </div>
      {actions ? (
        <div className={cn("flex justify-end pr-1 pt-1 transition-opacity", actionsClassName)}>
          {actions}
        </div>
      ) : null}
    </div>
  );
}

export function ChatAssistantTurn({ children, actions, containerRef, className }: ChatTurnProps) {
  return (
    <div
      ref={containerRef}
      className={cn(
        "group px-0 py-4 md:p-4 landscape-short:px-2 landscape-short:py-1.5",
        className
      )}
    >
      <div className="mx-auto flex w-full max-w-4xl flex-col gap-2 md:flex-row md:items-start md:gap-3">
        <div className="flex h-8 shrink-0 items-center gap-2 px-0 md:h-auto md:flex-col md:items-start md:gap-3 landscape-short:h-6">
          <MapleChatAvatar />
          <div className="text-sm font-semibold leading-none md:hidden">Maple</div>
        </div>
        <div className="flex min-w-0 w-full flex-1 flex-col overflow-hidden px-2 md:gap-2 md:px-0">
          <div className="hidden md:block">
            <div className="text-left text-sm font-semibold leading-none">Maple</div>
          </div>
          <div className="space-y-2">
            {children}
            {actions ? <div className="flex gap-1">{actions}</div> : null}
          </div>
        </div>
      </div>
    </div>
  );
}

export function ChatAssistantPendingTurn() {
  return (
    <ChatAssistantTurn>
      <div className="flex items-center gap-1" role="status" aria-label="Maple is responding">
        <div className="h-2 w-2 animate-pulse rounded-full bg-foreground/60" />
        <div className="h-2 w-2 animate-pulse rounded-full bg-foreground/60 delay-75" />
        <div className="h-2 w-2 animate-pulse rounded-full bg-foreground/60 delay-150" />
      </div>
    </ChatAssistantTurn>
  );
}

export function ChatDesktopConversationHeader({
  title,
  isSidebarOpen,
  onNewChat,
  titleClassName
}: {
  title: string;
  isSidebarOpen: boolean;
  onNewChat: () => void;
  titleClassName?: string;
}) {
  return (
    <div className="flex h-14 items-center px-4">
      <div className="relative flex flex-1 items-center justify-center">
        <h1
          className={cn(
            "max-w-[20rem] truncate text-base font-medium text-foreground transition-colors duration-300",
            titleClassName
          )}
        >
          {title}
        </h1>
        {!isSidebarOpen ? (
          <Button
            variant="outline"
            size="icon"
            className="absolute right-0 h-9 w-9 border-0"
            onClick={onNewChat}
            aria-label="New chat"
          >
            <SquarePen className="h-4 w-4" />
          </Button>
        ) : null}
      </div>
    </div>
  );
}

export const CHAT_COMPOSER_TEXTAREA_CLASS =
  "w-full min-h-[52px] landscape-short:min-h-[40px] max-h-[200px] landscape-short:max-h-[100px] resize-none border-0 bg-transparent py-3.5 landscape-short:py-2 pl-4 pr-2 leading-6 focus-visible:ring-0 focus-visible:ring-offset-0 placeholder:text-muted-foreground/60";

export function ChatComposerSurface({
  children,
  className
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      className={cn(
        "relative w-full overflow-hidden rounded-3xl border border-[hsl(var(--maple-secondary-container))] bg-background transition-colors focus-within:border-[hsl(var(--maple-primary))]",
        className
      )}
    >
      {children}
    </div>
  );
}
