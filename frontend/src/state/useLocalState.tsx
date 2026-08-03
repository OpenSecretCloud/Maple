import { useContext } from "react";
import {
  BillingStateContext,
  ModelStateContext,
  SelectedProjectStateContext,
  SidebarSearchStateContext
} from "./LocalStateContextDef";

export const useModelState = () => {
  const context = useContext(ModelStateContext);
  if (context === undefined) {
    throw new Error("useModelState must be used within a LocalStateProvider");
  }
  return context;
};

export const useBillingState = () => {
  const context = useContext(BillingStateContext);
  if (context === undefined) {
    throw new Error("useBillingState must be used within a LocalStateProvider");
  }
  return context;
};

export const useSidebarSearchState = () => {
  const context = useContext(SidebarSearchStateContext);
  if (context === undefined) {
    throw new Error("useSidebarSearchState must be used within a LocalStateProvider");
  }
  return context;
};

export const useSelectedProjectState = () => {
  const context = useContext(SelectedProjectStateContext);
  if (context === undefined) {
    throw new Error("useSelectedProjectState must be used within a LocalStateProvider");
  }
  return context;
};
