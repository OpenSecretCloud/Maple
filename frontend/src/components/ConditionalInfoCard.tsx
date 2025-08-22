import { Card, CardHeader } from "@/components/ui/card";
import { InfoContent } from "./Explainer";
import { useChatCount } from "@/hooks/useChatCount";

export function ConditionalInfoCard() {
  const { hasMinChats } = useChatCount();

  return (
    <>
      {!hasMinChats && (
        <Card className="bg-card/80 backdrop-blur-sm">
          <CardHeader>
            <InfoContent />
          </CardHeader>
        </Card>
      )}
      {hasMinChats && (
        <>
          {/* Desktop: Maintain empty space of same size */}
          <div className="hidden md:block">
            <Card className="bg-transparent border-transparent shadow-none">
              <CardHeader className="py-[88px]">{/* Empty space to maintain layout */}</CardHeader>
            </Card>
          </div>
          {/* Mobile: No empty space */}
          <div className="md:hidden"></div>
        </>
      )}
    </>
  );
}
