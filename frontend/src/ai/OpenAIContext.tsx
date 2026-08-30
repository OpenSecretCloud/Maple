import OpenAI from "openai";
import { useOpenSecret } from "@opensecret/react";
import { OpenAIContext } from "./OpenAIContextDef";

export const OpenAIProvider = ({ children }: { children: React.ReactNode }) => {
  const url = import.meta.env.VITE_OPEN_SECRET_API_URL;
  if (!url) {
    throw new Error("VITE_OPEN_SECRET_API_URL must be set");
  }

  const { aiCustomFetch, auth } = useOpenSecret();

  // V2 credentials are deliberately opaque to Maple. The provider's verified
  // user state, rather than a legacy storage key, determines readiness.
  if (auth.loading || !auth.user) {
    return <OpenAIContext.Provider value={undefined}>{children}</OpenAIContext.Provider>;
  }

  // Custom fetch function that allows us to refresh the access token
  const openai = new OpenAI({
    baseURL: `${url}/v1/`,
    dangerouslyAllowBrowser: true,
    apiKey: "not-a-real-api-key",
    defaultHeaders: {
      "Accept-Encoding": "identity"
    },
    fetch: aiCustomFetch,
    maxRetries: 0 // Disable automatic retries
  });

  return <OpenAIContext.Provider value={openai}>{children}</OpenAIContext.Provider>;
};

export { OpenAIContext } from "./OpenAIContextDef";
