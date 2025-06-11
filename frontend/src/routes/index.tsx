import ChatBox from "@/components/ChatBox";
import { BillingStatus } from "@/components/BillingStatus";

import { createFileRoute, useNavigate, Link } from "@tanstack/react-router";
import { useOpenSecret } from "@opensecret/react";
import { useLocalState } from "@/state/useLocalState";
import { Sidebar, SidebarToggle } from "@/components/Sidebar";
import { cva } from "class-variance-authority";
import { InfoContent } from "@/components/Explainer";
import { useState, useCallback, useEffect } from "react";
import { Card, CardHeader } from "@/components/ui/card";
import { VerificationModal } from "@/components/VerificationModal";
import { TopNav } from "@/components/TopNav";
import { Marketing } from "@/components/Marketing";
import { SimplifiedFooter } from "@/components/SimplifiedFooter";

const homeVariants = cva("grid h-full w-full overflow-hidden", {
  variants: {
    sidebar: {
      true: "grid-cols-1 md:grid-cols-[280px_1fr]",
      false: "grid-cols-1"
    }
  },
  defaultVariants: {
    sidebar: false
  }
});

type IndexSearchOptions = {
  login?: string;
  next?: string;
};

function validateSearch(search: Record<string, unknown>): IndexSearchOptions {
  return {
    login: search?.login === "true" ? "true" : undefined,
    next: search.next ? (search.next as string) : undefined
  };
}

export const Route = createFileRoute("/")({
  component: Index,
  validateSearch
});

function Index() {
  const navigate = useNavigate();
  const localState = useLocalState();

  const { login, next } = Route.useSearch();

  const os = useOpenSecret();

  useEffect(() => {
    if (login === "true") {
      navigate({
        to: "/login",
        search: next ? { next } : undefined
      });
    }
  }, [login, next, navigate]);

  const [isSidebarOpen, setIsSidebarOpen] = useState(false);

  const toggleSidebar = useCallback(() => setIsSidebarOpen((prev) => !prev), []);

  async function handleSubmit(input: string, systemPrompt?: string, images?: File[]) {
    if (input.trim() === "") return;
    localState.setUserPrompt(input.trim());
    localState.setSystemPrompt(systemPrompt?.trim() || null);
    localState.setUserImages(images || []);
    const id = await localState.addChat();
    navigate({ to: "/chat/$chatId", params: { chatId: id } });
  }

  return (
    <div className={homeVariants({ sidebar: os.auth.user !== undefined })}>
      {os.auth.user && (
        <Sidebar chatId={undefined} isOpen={isSidebarOpen} onToggle={toggleSidebar} />
      )}
      <main className="flex flex-col items-center gap-8 px-4 sm:px-8 py-16 h-full overflow-y-auto">
        {os.auth.user && !isSidebarOpen && (
          <div className="fixed top-4 left-4 z-20 md:hidden">
            <SidebarToggle onToggle={toggleSidebar} />
          </div>
        )}

        {!os.auth.user && <TopNav />}
        {os.auth.user ? (
          <>
            <div className="w-full max-w-[35rem] flex flex-col gap-8 ">
              <div className="flex flex-col items-center gap-2">
                <Link to="/">
                  <img
                    src="/maple-logo.svg"
                    alt="Maple AI logo"
                    className="w-[10rem] hidden dark:block"
                  />
                  <img
                    src="/maple-logo-dark.svg"
                    alt="Maple AI logo"
                    className="w-[10rem] block dark:hidden filter drop-shadow-sm"
                  />
                </Link>
                <h2 className="text-2xl font-light leading-none tracking-tight text-center text-balance text-foreground dark:text-white">
                  Private AI Chat
                </h2>
              </div>
              <div className="self-center">
                <BillingStatus />
              </div>
              <div className="col-span-3">
                {os.auth.user && <ChatBox startTall={true} onSubmit={handleSubmit} />}
              </div>
              <Card className="bg-card/80 backdrop-blur-sm">
                <CardHeader>
                  <InfoContent />
                </CardHeader>
              </Card>
              <SimplifiedFooter />
            </div>
          </>
        ) : (
          <Marketing />
        )}

        <VerificationModal />
      </main>
    </div>
  );
}
