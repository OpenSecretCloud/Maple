import { invoke } from "@tauri-apps/api/core";

const US_CODES = ["USA", "ASM", "GUM", "PRI", "VIR", "MNP", "UMI"]; // US and territories

/**
 * Gets the raw App Store region code.
 * Returns the region code, or a short error message if there's an error invoking the plugin.
 * Note: The Swift code returns "UNKNOWN" if it can't determine the region.
 */
export async function getStoreRegion(): Promise<string> {
  console.log("[Region Gate] Attempting to invoke plugin:store|get_region...");
  try {
    const code = await invoke<string>("plugin:store|get_region");
    console.log("[Region Gate] Success! Store region code:", code);
    return code;
  } catch (error) {
    console.error("[Region Gate] Error invoking plugin:", error);
    console.error("[Region Gate] Stack trace:", new Error().stack);

    // Return the error message for diagnosis
    let errorMsg = "Error: ";
    if (error instanceof Error) {
      errorMsg += error.message;
    } else if (typeof error === "string") {
      errorMsg += error;
    } else {
      errorMsg += JSON.stringify(error);
    }

    // Truncate if the error message is too long
    if (errorMsg.length > 30) {
      errorMsg = errorMsg.substring(0, 27) + "...";
    }

    return errorMsg;
  }
}

/**
 * Checks if external billing is allowed based on the App Store region.
 * Returns true for US regions, false for all others or on errors.
 */
export async function allowExternalBilling(): Promise<boolean> {
  const regionCode = await getStoreRegion();
  const isAllowed = US_CODES.includes(regionCode);
  console.log("[Region Gate] Is US region:", isAllowed, "Valid US codes:", US_CODES);
  return isAllowed;
}

/**
 * Checks if a region code is in the US.
 */
export function isUSRegion(regionCode: string): boolean {
  return US_CODES.includes(regionCode);
}
