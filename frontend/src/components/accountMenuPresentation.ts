export function getAccountMenuPresentation({
  compactSettingsLayout,
  pagePresentation
}: {
  compactSettingsLayout: boolean;
  pagePresentation: boolean;
}): {
  settingsPath: "/settings" | "/settings/account";
  controlSizeClass: string;
  iconSizeClass: string;
} {
  return {
    settingsPath: compactSettingsLayout ? "/settings" : "/settings/account",
    controlSizeClass: pagePresentation ? "h-11 w-11" : "h-9 w-9",
    iconSizeClass: pagePresentation ? "h-5 w-5" : "h-4 w-4"
  };
}
