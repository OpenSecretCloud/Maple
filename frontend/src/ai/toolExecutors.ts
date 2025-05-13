// Zod can be used for validation if needed
// import { z } from "zod"; // already in Maple deps

// Define more flexible input types to handle string or number inputs
type NumberInput = number | string;

// Helper function to ensure values are treated as numbers
function ensureNumber(value: NumberInput): number {
  return typeof value === "string" ? parseFloat(value) : value;
}

export const toolExecutors = {
  add: (args: { a: NumberInput; b: NumberInput }) => {
    const numA = ensureNumber(args.a);
    const numB = ensureNumber(args.b);
    return { result: numA + numB };
  },
  subtract: (args: { a: NumberInput; b: NumberInput }) => {
    const numA = ensureNumber(args.a);
    const numB = ensureNumber(args.b);
    return { result: numA - numB };
  }
} as const;

export type ToolName = keyof typeof toolExecutors;
