import { useContext } from "react";
import { OpenAIContext } from "./OpenAIContext";
import OpenAI from "openai";
import type { OpenAIContextType } from "./OpenAIContextDef";

// I do this is a separate file because of some dumb eslint
export const useOpenAI = (): OpenAI => {
  const context = useContext(OpenAIContext);
  if (context === undefined) {
    throw new Error("useOpenAI must be used within an OpenAIProvider");
  }
  if (context.client === undefined) {
    throw new Error(
      "OpenAI client is not initialized. Make sure you have provided valid credentials to the OpenAIProvider."
    );
  }
  return context.client;
};

export const useOpenAIContext = (): OpenAIContextType => {
  const context = useContext(OpenAIContext);
  if (context === undefined) {
    throw new Error("useOpenAIContext must be used within an OpenAIProvider");
  }
  return context;
};
