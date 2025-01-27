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
    <OpenSecretProvider
      apiUrl={import.meta.env.VITE_OPEN_SECRET_API_URL}
      pcrConfig={{
        pcr0Values: [
          "ed9109c16f30a470cf0ea2251816789b4ffa510c990118323ce94a2364b9bf05bdb8777959cbac86f5cabc4852e0da71",
          "4f2bcdf16c38842e1a45defd944d24ea58bb5bcb76491843223022acfe9eb6f1ff79b2cb9a6b2a9219daf9c7bf40fa37",
          "b8ee4b511ef2c9c6ab3e5c0840c5df2218fbb4d9df88254ece7af9462677e55aa5a03838f3ae432d86ca1cb6f992eee7"
        ],
        pcr0DevValues: [
          "799600ba64a29e360b1651f4ced6c9ca5323094a45294551327b996062c3f21e6fef651e7e3d97ec8d25be87b9935b4f",
          "2fd9d4f716fd28336d96bc1a20b18a727c2d18f292577ba99323acfc8fb08959428a123b7acff478994c4f961247a0c7",
          "4292db2a90ce5ea6f6e2766e0238a328c81dc060a1f3175bced2e94a10e0490d3ff9125d774dafdff969ac661778e757"
        ]
      }}
    >
      <LocalStateProvider>
        <OpenAIProvider>
          <QueryClientProvider client={queryClient}>
            <TooltipProvider>
              <BillingServiceProvider>
                <InnerApp />
              </BillingServiceProvider>
            </TooltipProvider>
          </QueryClientProvider>
        </OpenAIProvider>
      </LocalStateProvider>
    </OpenSecretProvider>
  );
}
