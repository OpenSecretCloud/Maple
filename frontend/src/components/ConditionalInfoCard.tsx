import { Card, CardHeader } from "@/components/ui/card";
import { InfoContent } from "./Explainer";
import { useChatCount } from "@/hooks/useChatCount";

export function ConditionalInfoCard() {
  const { hasMinChats, isLoading } = useChatCount();

  // While loading, render transparent placeholder to maintain footer position
  if (isLoading) {
    return (
      <>
        {/* Desktop: Transparent placeholder with same height */}
        <div className="hidden md:block" aria-hidden="true">
          <Card className="bg-transparent border-transparent shadow-none">
            <CardHeader className="py-[88px]" />
          </Card>
        </div>
        {/* Mobile: No placeholder needed */}
      </>
    );
  }

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
          <div className="hidden md:block" aria-hidden="true">
            <Card className="bg-transparent border-transparent shadow-none">
              <CardHeader className="py-[88px]" />
            </Card>
          </div>
          {/* Mobile: No empty space */}
        </>
      )}
    </>
  );
}
