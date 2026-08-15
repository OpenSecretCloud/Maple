import { useCallback, useMemo, useRef, useState } from "react";
import { BillingStatus } from "@/billing/billingApi";
import {
  BillingStateContext,
  ModelStateContext,
  OpenSecretModel,
  OpenSecretModelAlias,
  SelectedProjectStateContext,
  SidebarSearchStateContext,
  type BillingState,
  type ModelState,
  type SelectedProjectState,
  type SidebarSearchState
} from "./LocalStateContextDef";
import { aliasModelName, migrateStickyModelName } from "@/utils/utils";

export const QUICK_MODEL_ALIAS = "auto:quick";
export const POWERFUL_MODEL_ALIAS = "auto:powerful";
export const DEFAULT_MODEL_ID = QUICK_MODEL_ALIAS;
export const PAID_DEFAULT_MODEL_ID = POWERFUL_MODEL_ALIAS;
const SELECTED_MODEL_METADATA_KEY = "selectedModelMetadata";
type LocalStateStorage = Pick<Storage, "getItem" | "setItem" | "removeItem">;

function getBrowserStorage(): LocalStateStorage | null {
  if (typeof window === "undefined") return null;

  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

const DEFAULT_MODEL_ALIASES: OpenSecretModelAlias[] = [
  {
    id: QUICK_MODEL_ALIAS,
    label: "Quick",
    short_name: "Quick",
    description: "Fast, everyday responses",
    target_model: "",
    access: "free",
    capabilities: { chat: true, vision: false, reasoning: true, tool_use: true }
  },
  {
    id: POWERFUL_MODEL_ALIAS,
    label: "Powerful",
    short_name: "Powerful",
    description: "Deeper thinking & analysis",
    target_model: "",
    access: "pro",
    capabilities: { chat: true, vision: true, reasoning: true, tool_use: true }
  }
];

// Check if a plan name corresponds to a pro/max/team plan
function isProMaxOrTeamPlan(planName: string): boolean {
  return planName.includes("pro") || planName.includes("max") || planName.includes("team");
}

// Check if paid defaults have already been applied for this user.
// The value is an ISO date string indicating when defaults were last applied.
function hasPaidDefaultsBeenApplied(storage: LocalStateStorage | null): boolean {
  return storage?.getItem("paidDefaultsApplied") != null;
}

// One-time migration: clear stale webSearchEnabled values that were auto-persisted
// by the old useEffect (not explicit user choices). After this migration, only values
// written by explicit user toggle clicks are trusted.
function migrateWebSearchDefault(storage: LocalStateStorage | null): void {
  if (!storage) return;

  try {
    if (storage.getItem("webSearchDefaultMigrated") === null) {
      storage.removeItem("webSearchEnabled");
      storage.setItem("webSearchDefaultMigrated", "true");
    }
  } catch {
    // Ignore storage errors
  }
}

// Helper to get the initial web search state from localStorage.
// Web search is on by default for all users, but respects the user's explicit preference.
export function getInitialWebSearchEnabled(
  storage: LocalStateStorage | null = getBrowserStorage()
): boolean {
  migrateWebSearchDefault(storage);
  try {
    // If user has explicitly toggled web search before, respect that
    const webSearchSetting = storage?.getItem("webSearchEnabled");
    if (webSearchSetting != null) {
      return webSearchSetting === "true";
    }
  } catch (error) {
    console.error("Failed to get initial web search state:", error);
  }
  // Default to enabled for all users
  return true;
}

// Helper to get default model based on cached billing status
function getInitialModel(storage: LocalStateStorage | null): string {
  // Check for dev override first
  if (import.meta.env.VITE_DEV_MODEL_OVERRIDE) {
    return aliasModelName(import.meta.env.VITE_DEV_MODEL_OVERRIDE);
  }

  try {
    // Priority 1: Check local storage for user's explicit model choice
    const selectedModel = storage?.getItem("selectedModel");
    if (selectedModel) {
      if (getCachedSelectedModelMetadata(selectedModel, storage)) {
        return selectedModel;
      }

      return migrateStickyModelName(selectedModel);
    }

    // Priority 2: Check if paid defaults were already applied
    // (user is returning paid user who got the one-time flip but then
    // cleared selectedModel somehow — unlikely but safe fallback)
    if (hasPaidDefaultsBeenApplied(storage)) {
      return PAID_DEFAULT_MODEL_ID;
    }

    // Priority 3: Check cached billing status for pro/max/team users
    const cachedBillingStr = storage?.getItem("cachedBillingStatus");
    if (cachedBillingStr) {
      const cachedBilling = JSON.parse(cachedBillingStr) as BillingStatus;
      const planName = cachedBilling.product_name?.toLowerCase() || "";

      // Pro, Max, or Team users get the powerful reasoning model
      if (isProMaxOrTeamPlan(planName)) {
        return PAID_DEFAULT_MODEL_ID;
      }
    }
  } catch (error) {
    console.error("Failed to load initial model:", error);
  }

  // Priority 4: Default to free model
  return DEFAULT_MODEL_ID;
}

function normalizeAvailableModels(models: OpenSecretModel[]): OpenSecretModel[] {
  const normalizedModels = new Map<string, OpenSecretModel>();

  for (const model of models) {
    if (!normalizedModels.has(model.id)) {
      normalizedModels.set(model.id, model);
    }
  }

  return Array.from(normalizedModels.values());
}

function isAutoModelAlias(modelId: string): boolean {
  return modelId === QUICK_MODEL_ALIAS || modelId === POWERFUL_MODEL_ALIAS;
}

function getCachedSelectedModelMetadata(
  modelId: string,
  storage: LocalStateStorage | null
): OpenSecretModel | null {
  if (!modelId || isAutoModelAlias(modelId)) return null;

  try {
    const cachedMetadata = storage?.getItem(SELECTED_MODEL_METADATA_KEY);
    if (!cachedMetadata) return null;

    const parsedMetadata = JSON.parse(cachedMetadata) as OpenSecretModel;
    if (parsedMetadata.id !== modelId) return null;

    return {
      ...parsedMetadata,
      object: "model",
      created: parsedMetadata.created || Date.now(),
      owned_by: parsedMetadata.owned_by || "opensecret"
    };
  } catch (error) {
    console.error("Failed to load selected model metadata:", error);
    return null;
  }
}

function cacheSelectedModelMetadata(
  modelId: string,
  storage: LocalStateStorage | null,
  modelMetadata?: OpenSecretModel | null
) {
  if (!storage) return;

  try {
    if (!modelMetadata || isAutoModelAlias(modelId)) {
      storage.removeItem(SELECTED_MODEL_METADATA_KEY);
      return;
    }

    const cacheableMetadata: OpenSecretModel = {
      ...modelMetadata,
      id: modelId,
      object: "model",
      created: modelMetadata.created || Date.now(),
      owned_by: modelMetadata.owned_by || "opensecret"
    };

    storage.setItem(SELECTED_MODEL_METADATA_KEY, JSON.stringify(cacheableMetadata));
  } catch (error) {
    console.error("Failed to cache selected model metadata:", error);
  }
}

export const LocalStateProvider = ({
  children,
  storage = getBrowserStorage()
}: {
  children: React.ReactNode;
  storage?: LocalStateStorage | null;
}) => {
  const [modelState, setModelState] = useState(() => {
    const model = getInitialModel(storage);
    const cachedSelectedModel = getCachedSelectedModelMetadata(model, storage);

    return {
      model,
      availableModels: cachedSelectedModel ? [cachedSelectedModel] : ([] as OpenSecretModel[]),
      modelAliases: DEFAULT_MODEL_ALIASES,
      hasWhisperModel: true // Default to true to avoid hiding button during loading
    };
  });
  const [billingState, setBillingState] = useState<{
    status: BillingStatus | null;
    accountId: string | null;
  }>({ status: null, accountId: null });
  const billingStatus = billingState.status;
  const [searchQuery, setSearchQueryState] = useState("");
  const [isSearchVisible, setIsSearchVisibleState] = useState(false);
  const [selectedProjectId, setSelectedProjectIdState] = useState<string | null>(null);
  const currentModelRef = useRef(modelState.model);

  // Internal model setter — updates state and localStorage but does NOT mark as
  // a user's explicit choice. Used by billing/system logic.
  const setModelInternal = useCallback(
    (modelId: string, persist = false) => {
      const aliasedModel = aliasModelName(modelId);
      currentModelRef.current = aliasedModel;
      setModelState((prev) => {
        if (prev.model === aliasedModel) return prev;
        return { ...prev, model: aliasedModel };
      });
      if (persist) {
        try {
          storage?.setItem("selectedModel", aliasedModel);
          cacheSelectedModelMetadata(aliasedModel, storage);
        } catch (error) {
          console.error("Failed to save model to localStorage:", error);
        }
      }
    },
    [storage]
  );

  const setBillingStatus = useCallback(
    (status: BillingStatus, accountId: string | null = null) => {
      setBillingState({ status, accountId });

      const planName = status.product_name?.toLowerCase() || "";
      const isPaidPlan =
        planName.includes("pro") || planName.includes("max") || planName.includes("team");

      const isProMaxOrTeam = isProMaxOrTeamPlan(planName);

      // Check if billing plan changed from cached version
      let billingChanged = false;
      try {
        const cachedBillingStr = storage?.getItem("cachedBillingStatus");
        if (cachedBillingStr) {
          const cachedBilling = JSON.parse(cachedBillingStr) as BillingStatus;
          const cachedPlan = cachedBilling.product_name?.toLowerCase() || "";
          billingChanged = cachedPlan !== planName;
        }
      } catch (error) {
        console.error("Failed to check cached billing:", error);
      }

      // Cache billing status to localStorage only for paid users
      try {
        if (isPaidPlan) {
          storage?.setItem("cachedBillingStatus", JSON.stringify(status));
        } else {
          // Clear cache for free users
          storage?.removeItem("cachedBillingStatus");
        }
      } catch (error) {
        console.error("Failed to cache billing status:", error);
      }

      // One-time paid defaults: when a user is on pro/max/team and we haven't
      // applied paid defaults yet, flip model to "Powerful" and web search ON.
      // This handles both new signups and free-to-paid upgrades.
      try {
        if (storage && isProMaxOrTeam && !hasPaidDefaultsBeenApplied(storage)) {
          // Apply paid defaults — set model to Powerful reasoning model
          setModelInternal(PAID_DEFAULT_MODEL_ID, true);

          // Mark when we applied paid defaults (ISO date) so we never override again.
          // Future defaults can check this date to decide whether to re-apply newer defaults.
          storage?.setItem("paidDefaultsApplied", new Date().toISOString());

          return;
        }
      } catch (error) {
        console.error("Failed to apply paid defaults:", error);
      }

      // For users who already had defaults applied: handle plan changes
      try {
        if (billingChanged) {
          if (isProMaxOrTeam) {
            // Plan changed but still pro-tier — only update model if user
            // hasn't manually chosen one (selectedModel not in localStorage)
            const selectedModel = storage?.getItem("selectedModel");
            if (!selectedModel) {
              setModelInternal(PAID_DEFAULT_MODEL_ID, true);
            }
          } else {
            // User downgraded to free — switch back to free model
            // and clear paid defaults so they get re-applied if they upgrade again
            setModelInternal(DEFAULT_MODEL_ID);
            storage?.removeItem("paidDefaultsApplied");
            storage?.removeItem("selectedModel");
          }
        }
      } catch (error) {
        console.error("Failed to update model based on billing status:", error);
      }
    },
    [setModelInternal, storage]
  );

  const clearBillingStatus = useCallback(() => {
    setBillingState((current) =>
      current.status === null && current.accountId === null
        ? current
        : { status: null, accountId: null }
    );
  }, []);

  const setSearchQuery = useCallback((query: string) => setSearchQueryState(query), []);

  const setIsSearchVisible = useCallback(
    (visible: boolean) => setIsSearchVisibleState(visible),
    []
  );

  const setSelectedProjectId = useCallback((projectId: string | null) => {
    setSelectedProjectIdState(projectId);
  }, []);

  // Public model setter — records the choice as a user-initiated selection.
  // After this, we won't auto-override their model choice.
  const setModel = useCallback(
    (model: string, modelMetadata?: OpenSecretModel | null) => {
      const nextModel = modelMetadata ? model : aliasModelName(model);
      if (currentModelRef.current === nextModel && !modelMetadata) return;
      currentModelRef.current = nextModel;

      // Save to localStorage as user's explicit choice. Keep this outside the
      // state updater because React may replay updater functions.
      try {
        storage?.setItem("selectedModel", nextModel);
        cacheSelectedModelMetadata(nextModel, storage, modelMetadata);
      } catch (error) {
        console.error("Failed to save model to localStorage:", error);
      }

      setModelState((prev) => {
        const availableModels =
          modelMetadata && !isAutoModelAlias(nextModel)
            ? normalizeAvailableModels([modelMetadata, ...prev.availableModels])
            : prev.availableModels;

        return { ...prev, model: nextModel, availableModels };
      });
    },
    [storage]
  );

  const setAvailableModels = useCallback((models: OpenSecretModel[]) => {
    setModelState((prev) => ({
      ...prev,
      availableModels: normalizeAvailableModels(models)
    }));
  }, []);

  const setModelAliases = useCallback((aliases: OpenSecretModelAlias[]) => {
    setModelState((prev) => ({
      ...prev,
      modelAliases: aliases.length > 0 ? aliases : DEFAULT_MODEL_ALIASES
    }));
  }, []);

  const setHasWhisperModel = useCallback((hasWhisper: boolean) => {
    setModelState((prev) => ({ ...prev, hasWhisperModel: hasWhisper }));
  }, []);

  const modelValue = useMemo<ModelState>(
    () => ({
      model: modelState.model,
      availableModels: modelState.availableModels,
      modelAliases: modelState.modelAliases,
      setModel,
      setAvailableModels,
      setModelAliases,
      hasWhisperModel: modelState.hasWhisperModel,
      setHasWhisperModel
    }),
    [modelState, setAvailableModels, setHasWhisperModel, setModel, setModelAliases]
  );
  const billingValue = useMemo<BillingState>(
    () => ({
      billingStatus,
      billingStatusAccountId: billingState.accountId,
      setBillingStatus,
      clearBillingStatus
    }),
    [billingState.accountId, billingStatus, clearBillingStatus, setBillingStatus]
  );
  const sidebarSearchValue = useMemo<SidebarSearchState>(
    () => ({ searchQuery, setSearchQuery, isSearchVisible, setIsSearchVisible }),
    [isSearchVisible, searchQuery, setIsSearchVisible, setSearchQuery]
  );
  const selectedProjectValue = useMemo<SelectedProjectState>(
    () => ({ selectedProjectId, setSelectedProjectId }),
    [selectedProjectId, setSelectedProjectId]
  );

  return (
    <ModelStateContext.Provider value={modelValue}>
      <BillingStateContext.Provider value={billingValue}>
        <SidebarSearchStateContext.Provider value={sidebarSearchValue}>
          <SelectedProjectStateContext.Provider value={selectedProjectValue}>
            {children}
          </SelectedProjectStateContext.Provider>
        </SidebarSearchStateContext.Provider>
      </BillingStateContext.Provider>
    </ModelStateContext.Provider>
  );
};
