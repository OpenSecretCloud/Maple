import { Info } from "lucide-react";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { MODEL_CONFIG } from "@/components/ModelSelector";

export function ModelTooltip() {
  // Get all models from the MODEL_CONFIG
  const models = Object.entries(MODEL_CONFIG).map(([id, config]) => ({
    id,
    displayName: config.displayName,
    shortName: config.shortName,
    badges: config.badges || [],
    requiresPro: config.requiresPro,
    requiresStarter: config.requiresStarter
  }));

  // Sort models: Free models first, then starter, then pro, with some logical ordering
  const sortedModels = models.sort((a, b) => {
    // Free models (no requirements) first
    if (!a.requiresPro && !a.requiresStarter && (b.requiresPro || b.requiresStarter)) return -1;
    if (!b.requiresPro && !b.requiresStarter && (a.requiresPro || a.requiresStarter)) return 1;

    // Then starter models
    if (a.requiresStarter && !b.requiresStarter && !b.requiresPro) return 1;
    if (b.requiresStarter && !a.requiresStarter && !a.requiresPro) return -1;

    // Then pro models
    if (a.requiresPro && !b.requiresPro) return 1;
    if (b.requiresPro && !a.requiresPro) return -1;

    // Within same tier, sort alphabetically
    return a.displayName.localeCompare(b.displayName);
  });

  return (
    <TooltipProvider>
      <Tooltip>
        <TooltipTrigger asChild>
          <button className="inline-flex items-center justify-center w-4 h-4 ml-1 text-[hsl(var(--marketing-text-muted))] hover:text-foreground transition-colors">
            <Info className="w-4 h-4" />
            <span className="sr-only">View available models</span>
          </button>
        </TooltipTrigger>
        <TooltipContent className="max-w-xs p-4">
          <div className="space-y-3">
            <div className="font-medium text-sm">Available Models:</div>
            <div className="space-y-2">
              {sortedModels.map((model) => (
                <div key={model.id} className="flex items-start gap-2">
                  <div className="flex-1">
                    <div className="text-sm font-medium text-foreground">{model.displayName}</div>
                    {model.badges && model.badges.length > 0 && (
                      <div className="flex gap-1 mt-1">
                        {model.badges.map((badge) => (
                          <span
                            key={badge}
                            className="inline-flex items-center px-1.5 py-0.5 rounded text-xs font-medium bg-[hsl(var(--purple))]/10 text-[hsl(var(--purple))]"
                          >
                            {badge}
                          </span>
                        ))}
                      </div>
                    )}
                  </div>
                </div>
              ))}
            </div>
          </div>
        </TooltipContent>
      </Tooltip>
    </TooltipProvider>
  );
}
