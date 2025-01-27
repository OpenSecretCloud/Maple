import { useContext } from "react";
import { LocalStateContext } from "./LocalStateContext";

export const useLocalState = () => {
  const context = useContext(LocalStateContext);
  if (context === undefined) {
    throw new Error("useLocalState must be used within a LocalStateProvider");
  }
  return context;
};
