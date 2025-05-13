import { RotateCw, Calculator, ChevronRight, ArrowRight, Hash } from "lucide-react";
import { AssistantToolCall } from "@/state/LocalStateContextDef";
import { useState } from "react";

// For displaying tool call and its result
interface ToolCallMessageWithResultProps {
  call: AssistantToolCall;
  pending: boolean;
  result?: string; // Optional result for completed tool calls
}

export function ToolCallMessage({ call, pending, result }: ToolCallMessageWithResultProps) {
  const [expanded, setExpanded] = useState(false);

  // Safely parse arguments for display
  const parsedArgs = (() => {
    try {
      return JSON.parse(call.function.arguments || "{}");
    } catch {
      return {};
    }
  })();

  // Format result for display
  const parsedResult = (() => {
    if (!result) return undefined;

    try {
      const resultObj = JSON.parse(result);
      return resultObj.result !== undefined ? resultObj.result : result;
    } catch {
      return result;
    }
  })();

  // Format compact argument display
  const compactArgs = Object.entries(parsedArgs)
    .map(([key, value]) => `${key}: ${value}`)
    .join(", ");

  // Icon based on status
  const statusIcon = pending ? (
    <RotateCw className="animate-spin h-3.5 w-3.5 text-blue-400" />
  ) : (
    <Calculator className="h-3.5 w-3.5 text-indigo-400" />
  );

  // Class names based on state
  const containerClasses = `
    flex items-center gap-1.5 py-1.5 px-3 rounded-md
    font-mono text-xs transition-all
    ${expanded ? "bg-indigo-950/30 border border-indigo-800/40" : "bg-indigo-950/20 border border-indigo-900/30"}
    ${pending ? "opacity-80" : "opacity-100"}
    hover:bg-indigo-950/40 cursor-pointer
  `;

  return (
    <div className={containerClasses} onClick={() => setExpanded(!expanded)}>
      {statusIcon}

      <div className="flex flex-col gap-0.5 flex-grow">
        <div className="flex items-center gap-1.5">
          <span className="font-semibold text-indigo-300">tool.{call.function.name}()</span>

          {!pending && parsedResult !== undefined && (
            <>
              <ArrowRight className="h-3 w-3 text-muted-foreground" />
              <span className="text-yellow-300 font-semibold">
                {typeof parsedResult === "number" ? parsedResult : `${parsedResult}`}
              </span>
            </>
          )}
        </div>

        {expanded && (
          <div className="mt-1 pt-1 border-t border-indigo-800/30 text-[10px] text-muted-foreground">
            <div className="flex gap-1 items-center">
              <Hash className="h-3 w-3" />
              <span>params: {`{ ${compactArgs} }`}</span>
            </div>

            {!pending && parsedResult !== undefined && (
              <div className="flex gap-1 items-center mt-0.5">
                <ArrowRight className="h-3 w-3" />
                <span>
                  result: {typeof parsedResult === "number" ? parsedResult : `${parsedResult}`}
                </span>
              </div>
            )}
          </div>
        )}
      </div>

      <ChevronRight
        className={`h-3 w-3 text-muted-foreground transition-transform ${expanded ? "rotate-90" : ""}`}
      />
    </div>
  );
}
