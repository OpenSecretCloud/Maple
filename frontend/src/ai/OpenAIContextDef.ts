import { createContext } from "react";
import OpenAI from "openai";

export const OpenAIContext = createContext<OpenAI | undefined>(undefined);
