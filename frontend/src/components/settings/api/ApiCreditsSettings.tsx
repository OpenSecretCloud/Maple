import { ApiCreditsSection } from "@/components/apikeys/ApiCreditsSection";

type ApiCreditsSettingsProps = {
  showCreditSuccessMessage?: boolean;
};

export function ApiCreditsSettings({ showCreditSuccessMessage = false }: ApiCreditsSettingsProps) {
  return <ApiCreditsSection showSuccessMessage={showCreditSuccessMessage} />;
}
