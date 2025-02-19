import OpenAI from "openai";
import { useOpenSecret } from "@opensecret/react";
import { OpenAIContext } from "./OpenAIContextDef";

export const OpenAIProvider = ({ children }: { children: React.ReactNode }) => {
  const url = import.meta.env.VITE_OPEN_SECRET_API_URL;
  if (!url) {
    throw new Error("VITE_OPEN_SECRET_API_URL must be set");
  }

  const { aiCustomFetch } = useOpenSecret();

  const access_token = window.localStorage.getItem("access_token");

  // If we're not logged in we can't set up openai
  if (!access_token) {
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
    fetch: aiCustomFetch
  });

  return <OpenAIContext.Provider value={openai}>{children}</OpenAIContext.Provider>;
};

export { OpenAIContext } from "./OpenAIContextDef";
