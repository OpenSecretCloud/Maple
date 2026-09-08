import { cn } from "@/utils/utils";
import { Footer } from "./Footer";

interface FullPageMainProps {
  children: React.ReactNode;
  className?: string;
}

export function FullPageMain({ children, className }: FullPageMainProps) {
  return (
    <main
      className={cn(
        "flex flex-col items-center gap-8 px-4 sm:px-8 py-16 pt-28 overflow-y-aulo",
        className
      )}
    >
      <div className="flex flex-col gap-8 w-full">
        {children}
        <div>
          <Footer />
        </div>
      </div>
    </main>
  );
}
