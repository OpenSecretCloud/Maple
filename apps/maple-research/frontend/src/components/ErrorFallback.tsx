import { Card, CardContent, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Link } from "@tanstack/react-router";

export function ErrorFallback({ error }: { error: Error }) {
  console.error(error);
  return (
    <div className="mx-auto px-4 py-8 max-w-md">
      <Card className="bg-card/70 backdrop-blur-sm max-w-[45rem]">
        <CardHeader>
          <CardTitle>{error.name}</CardTitle>
        </CardHeader>
        <CardContent>{error.message}</CardContent>
        <CardFooter>
          <p>
            <Link className="underline" to="/">
              Go home
            </Link>
          </p>
        </CardFooter>
      </Card>
    </div>
  );
}
