import { useContext } from "react";
import { OpenAIContext } from "./OpenAIContext";
import OpenAI from "openai";

// I do this is a separate file because of some dumb eslint
export const useOpenAI = (): OpenAI => {
  const context = useContext(OpenAIContext);
  if (context === undefined) {
    throw new Error("useOpenAI must be used within an OpenAIProvider");
  }
  return context;
};
