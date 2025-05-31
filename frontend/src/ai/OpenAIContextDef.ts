import { createContext } from "react";
import OpenAI from "openai";

export interface OpenAIContextType {
  client: OpenAI | undefined;
}

export const OpenAIContext = createContext<OpenAIContextType>({
  client: undefined
});
