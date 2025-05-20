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
      clientId={import.meta.env.VITE_CLIENT_ID}
      pcrConfig={{
        pcr0Values: [
          "ed9109c16f30a470cf0ea2251816789b4ffa510c990118323ce94a2364b9bf05bdb8777959cbac86f5cabc4852e0da71",
          "4f2bcdf16c38842e1a45defd944d24ea58bb5bcb76491843223022acfe9eb6f1ff79b2cb9a6b2a9219daf9c7bf40fa37",
          "b8ee4b511ef2c9c6ab3e5c0840c5df2218fbb4d9df88254ece7af9462677e55aa5a03838f3ae432d86ca1cb6f992eee7",
          "33ffe5cae0f72cfe904bde8019ad98efa0ce5db2800f37c5d4149461023d1f70ea77e4f58ae1327ff46ed6a34045d6e2",
          "a1398fa2946b6ed4b96a1a992ee668aef3661329690f87d44cad5b646ce33e3b16a55674b1d6d54d115a5520801b97d6",
          "878dc4111e94722f3d33b202dc1368916af2eb486e74b3d94c9dfbcb3d981fa652827ea8e951ddfe06d1cefb482e431c",
          "4e242871fecc14933c889908a6a7593de574c2655a47ffa163c5fd7ba41d063152ef441bd555ac7f8569eac4fd7cbc8b",
          "095d38ba5c9c7ad1cfe5832d3dd8304b020392867aeef84f47e08b4305b867540b0ff5b2eb7d279de410e19ad937896e",
          "72c9a1dc207d919196c78f845c0f5fd4b3b3a690e024a3dd599f160be04875fbe52983773909a9f1584105f7d5103538",
          "a5ae21e211fa709dbced7fec7fff0eb86001174365a29271c07d7fb55fd9f37c7e2ddef1b01f39e977ca246277efeef2",
          "2520e5067830a34fc457b6360358e3754d53ab855f4a05a81312f2a2ed0bf893c5ec43d1325972efbe3a8f2b79303734",
          "a275a3877972c670c4f43e658cc9296838f79a96a26429877f74285c3a088d426583d3e2f6f99cfd96c70fdfa1475266",
          "8de5541089649e9edb2cd96fafb90716aa298483447e459708e8840b1f82a557c9d9ff6ae1fd2461b04310e7d9400d7d",
          "02a41da2df084fd1dee420d7717bef6dc0120f1d6a0b7fded3f4c7a539be4044b3061c71bc7156731db1fb66494097b0"
        ],
        pcr0DevValues: [
          "799600ba64a29e360b1651f4ced6c9ca5323094a45294551327b996062c3f21e6fef651e7e3d97ec8d25be87b9935b4f",
          "2fd9d4f716fd28336d96bc1a20b18a727c2d18f292577ba99323acfc8fb08959428a123b7acff478994c4f961247a0c7",
          "4292db2a90ce5ea6f6e2766e0238a328c81dc060a1f3175bced2e94a10e0490d3ff9125d774dafdff969ac661778e757",
          "f58409ae1bc8600c887fef5cc4055149c88c94b41c2b3e268826af7b43a1cdbacffdb2c96bf5972120c6460ab83fe89e",
          "6fcdb8086806a96c421c08eaf67cebf164aa898798b6f91b072c884773bc6ed64fe8f5af644fe35411195167b0e4a5f1",
          "0042958bde1fdd1bcbd4085ec94456c49e7bc5d2c3368f6f34edd6f339193cb7b53929d299eaf6a220ed5b7691f8618a",
          "583ac140e0454dd4766a07c147cb6d90d5430d6bc9c1571da19c781dea4027e1c434273caba584440180ca42c2db84d5",
          "4451e47ddb4be8a63492e62bc400e69d924188040805c658334f708e8682d308af3feb16018e98a5589c345d28437a6b",
          "5bc5a32791948dc7e315d01ec787307799bb6f70903d14c20dc47f19bb0ef3830eb3b2c5b04b7ae5b04717046b357a14",
          "4243170eeb11d38cf9bbee48b754bccfe97385b4639051efe97cda50086784cd32009dcb89a0fb1098558f22dc55b4e6",
          "be3de8fa74f42cc5165823da63aa283f1c8dfedb5e27e0bfa281c6dbd12d5b5bfd9bef591200175c44dbf0c504f5b0a5",
          "ab7e90e1894f75fbd423f3d0027973b611beff7402bd224dcd0f162968fb9678f08c92970d16a444edc934aa6ecd7d62",
          "8ba003e9d552d262cfd40a77eced199cfd0e776d1fd69d5ed6b7e6c6c92b91a0481e965247502f95a20478fe0a8f3de7",
          "2e6e8f86a657f6daed8e699c0b74bf02d6c7d6414638b1032e6addd2bca7d208a0e8aa4961eb8e926c54dc58fd63d401",
          "6b15f0571de13a6357e646bbe3772a8fe32fdd85b07ba97b3a6f95bdc43023dd9deb5710c26b75346de90157d9ecdd1f"
        ]
      }}
    >
      <LocalStateProvider>
        <OpenAIProvider>
          <QueryClientProvider client={queryClient}>
            <TooltipProvider>
              <BillingServiceProvider>
                <DeepLinkHandler />
                <InnerApp />
              </BillingServiceProvider>
            </TooltipProvider>
          </QueryClientProvider>
        </OpenAIProvider>
      </LocalStateProvider>
    </OpenSecretProvider>
  );
}
