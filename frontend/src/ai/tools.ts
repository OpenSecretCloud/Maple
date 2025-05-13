/** JSON schema that follows the vLLM/Privatemode spec */
export interface ToolSchema {
  type: "function";
  function: {
    name: string;
    description: string;
    parameters: {
      type: "object";
      properties: Record<string, unknown>;
      required: string[];
    };
  };
}

export const addTool: ToolSchema = {
  type: "function",
  function: {
    name: "add",
    description:
      "Add two numbers together. IMPORTANT: Your input parameters must be actual numbers (5, 8.2, etc.) NOT strings. Use JSON number literals without quotes.",
    parameters: {
      type: "object",
      properties: {
        a: {
          type: "number",
          description:
            'First number to add. Must be a number literal (e.g., 5, not "5"). No quotes.'
        },
        b: {
          type: "number",
          description:
            'Second number to add. Must be a number literal (e.g., 8, not "8"). No quotes.'
        }
      },
      required: ["a", "b"]
    }
  }
};

export const subtractTool: ToolSchema = {
  type: "function",
  function: {
    name: "subtract",
    description:
      "Subtract the second number from the first. IMPORTANT: Your input parameters must be actual numbers (5, 8.2, etc.) NOT strings. Use JSON number literals without quotes.",
    parameters: {
      type: "object",
      properties: {
        a: {
          type: "number",
          description:
            'First number (minuend). Must be a number literal (e.g., 5, not "5"). No quotes.'
        },
        b: {
          type: "number",
          description:
            'Number to subtract (subtrahend). Must be a number literal (e.g., 3, not "3"). No quotes.'
        }
      },
      required: ["a", "b"]
    }
  }
};

export const TOOL_DEFINITIONS: ToolSchema[] = [addTool, subtractTool];
