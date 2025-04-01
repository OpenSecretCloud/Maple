import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Link } from "@tanstack/react-router";
import { Footer } from "./Footer";

type AuthMainProps = {
  children: React.ReactNode;
  title: string;
  description?: string;
};

export function AuthMain({ children, title, description }: AuthMainProps) {
  return (
    <div className="flex flex-col min-h-screen">
      <main className="flex-1 flex flex-col items-center justify-center px-4 py-16">
        <Link to="/" className="h-[40px] flex items-center mb-8">
          <img
            src="/maple-logo-dark.svg"
            alt="Maple AI logo"
            className="w-[10rem] block dark:hidden"
            width={160}
            height={40}
          />
          <img
            src="/maple-logo.svg"
            alt="Maple AI logo"
            className="w-[10rem] hidden dark:block"
            width={160}
            height={40}
          />
        </Link>
        <Card className="bg-card/70 backdrop-blur-sm w-full max-w-md mb-8">
          <CardHeader className="pb-4">
            <CardTitle className="text-xl">{title}</CardTitle>
            {description && <CardDescription>{description}</CardDescription>}
          </CardHeader>
          <CardContent className="grid gap-4 pt-0">{children}</CardContent>
        </Card>
      </main>
      <div className="mt-auto">
        <Footer />
      </div>
    </div>
  );
}
