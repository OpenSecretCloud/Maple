import React, { useContext, useMemo } from "react";
import OpenAI from "openai";
import { OpenSecretContext, type OpenSecretContextType } from "@opensecret/react";
import { OpenAIContext } from "./OpenAIContextDef";

export const OpenAIProvider = ({ children }: { children: React.ReactNode }) => {
  const url = import.meta.env.VITE_OPEN_SECRET_API_URL;
  if (!url) {
    throw new Error("VITE_OPEN_SECRET_API_URL must be set");
  }

  const { aiCustomFetch } = useContext(OpenSecretContext as React.Context<OpenSecretContextType>);
  const access_token = window.localStorage.getItem("access_token");

  // Memoize the OpenAI client to prevent recreating it on every render
  const openai = useMemo(() => {
    // If we're not logged in or don't have aiCustomFetch, return undefined
    if (!access_token || !aiCustomFetch) {
      return undefined;
    }


    // Custom fetch function that allows us to refresh the access token
    return new OpenAI({
      baseURL: `${url}/v1/`,
      dangerouslyAllowBrowser: true,
      apiKey: "not-a-real-api-key",
      defaultHeaders: {
        "Accept-Encoding": "identity"
      },
      fetch: aiCustomFetch as any
    });
  }, [url, aiCustomFetch, access_token]);

  return <OpenAIContext.Provider value={openai}>{children}</OpenAIContext.Provider>;
};

export { OpenAIContext } from "./OpenAIContextDef";
