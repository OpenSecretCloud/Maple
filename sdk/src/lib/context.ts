import { useContext } from "react";
import { OpenSecretContext, OpenSecretContextType } from "./main";

export function useOpenSecret(): OpenSecretContextType {
  const context = useContext(OpenSecretContext);
  // React 19 compatibility: Don't check for nullish context since the default value is provided
  return context;
}
