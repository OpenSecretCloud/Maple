import "./index.css";
import "./chat.css";
import { TooltipProvider } from "@/components/ui/tooltip.tsx";
import { RouterProvider, createRouter } from "@tanstack/react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { routeTree } from "./routeTree.gen";
import { useOpenSecret, OpenSecretProvider } from "@opensecret/react";
import { OpenAIProvider } from "./ai/OpenAIContext";
import { LocalStateProvider } from "./state/LocalStateContext";
import { ErrorFallback } from "./components/ErrorFallback";
import { NotFoundFallback } from "./components/NotFoundFallback";
import { BillingServiceProvider } from "./components/BillingServiceProvider";
import { DeepLinkHandler } from "./components/DeepLinkHandler";
import { NotificationProvider } from "./contexts/NotificationContext";
import { ThemeProvider } from "./contexts/ThemeContext";
import { ProxyEventListener } from "./components/ProxyEventListener";
import { UpdateEventListener } from "./components/UpdateEventListener";
import { TTSProvider } from "./services/tts/TTSContext";
import { OPEN_SECRET_ATTESTATION_ENVIRONMENT } from "./config/attestation";

const DEFAULT_OPEN_SECRET_CLIENT_ID = "ba5a14b5-d915-47b1-b7b1-afda52bc5fc6";

// Create a new router instance
const router = createRouter({
  routeTree,
  context: {
    os: undefined! // This will be set after we wrap the app in an AuthProvider
  },
  defaultErrorComponent: ErrorFallback,
  defaultNotFoundComponent: NotFoundFallback
});

// Register the router instance for type safety
declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}

function InnerApp() {
  const os = useOpenSecret();
  return <RouterProvider router={router} context={{ os }} />;
}

const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: 0, // Consider data stale immediately
      refetchOnWindowFocus: false, // Don't refetch when window regains focus
      refetchOnMount: true, // Do refetch on component mount to get fresh data
      retry: false // Don't retry failed requests automatically
    }
  }
});

export default function App() {
  return (
    <ThemeProvider>
      <NotificationProvider>
        <OpenSecretProvider
          apiUrl={import.meta.env.VITE_OPEN_SECRET_API_URL}
          clientId={import.meta.env.VITE_CLIENT_ID || DEFAULT_OPEN_SECRET_CLIENT_ID}
          pcrConfig={{
            environment: OPEN_SECRET_ATTESTATION_ENVIRONMENT
          }}
        >
          <LocalStateProvider>
            <OpenAIProvider>
              <QueryClientProvider client={queryClient}>
                <TooltipProvider>
                  <TTSProvider>
                    <BillingServiceProvider>
                      <ProxyEventListener />
                      <UpdateEventListener />
                      <DeepLinkHandler />
                      <InnerApp />
                    </BillingServiceProvider>
                  </TTSProvider>
                </TooltipProvider>
              </QueryClientProvider>
            </OpenAIProvider>
          </LocalStateProvider>
        </OpenSecretProvider>
      </NotificationProvider>
    </ThemeProvider>
  );
}
