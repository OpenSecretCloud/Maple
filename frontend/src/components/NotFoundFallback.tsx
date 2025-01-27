import { Card, CardContent, CardFooter, CardHeader, CardTitle } from "@/components/ui/card";
import { Link, NotFoundRouteProps } from "@tanstack/react-router";

export function NotFoundFallback(props: NotFoundRouteProps) {
  console.error(props);
  return (
    <div className="mx-auto px-4 py-8 max-w-md">
      <Card className="bg-card/70 backdrop-blur-sm mx-auto max-w-[45rem]">
        <CardHeader>
          <CardTitle>404</CardTitle>
        </CardHeader>
        <CardContent>
          <p>Route not found.</p>
        </CardContent>
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
