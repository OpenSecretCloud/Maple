import { createFileRoute } from "@tanstack/react-router";
import { TopNav } from "@/components/TopNav";
import { FullPageMain } from "@/components/FullPageMain";
import { MarketingHeader } from "@/components/MarketingHeader";

export const Route = createFileRoute("/about")({
  component: About
});

function About() {
  return (
    <>
      <TopNav />
      <FullPageMain>
        <MarketingHeader title="Privacy is Natural" subtitle="Why we built Maple" />
        <div className="max-w-[35rem] self-center flex flex-col gap-8 border-white/10 bg-black/75 p-8 border rounded-lg text-lg font-light text-white/90">
          <p>
            Privacy isn't just a human value - it's a natural one. Maple trees communicate through
            underground fungal networks, sending messages and sharing resources invisibly. This
            discreet communication system allows them to thrive in challenging environments.
          </p>
          <p>
            At{" "}
            <a href="https://opensecret.cloud/" className="underline">
              OpenSecret
            </a>
            , we prioritize your privacy above all else. In an era of data breaches and
            surveillance, privacy grants individuals control over their lives. By extending this
            principle to our AI chat, we're empowering you to discuss sensitive topics with
            confidence.
          </p>
          <p>
            Our end-to-end encrypted AI chat lets you invite powerful AI into your innermost
            thoughts, with the knowledge that no one else - not even us - can access your
            conversations. This creates a more private, secure, and trustworthy world, one that puts
            the needs and values of individuals at its core.
          </p>
        </div>
      </FullPageMain>
    </>
  );
}
