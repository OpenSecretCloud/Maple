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
        <MarketingHeader
          title={
            <h2 className="text-6xl font-light mb-0">
              <span className="dark:text-[hsl(var(--blue))] text-[hsl(var(--purple))]">About</span>{" "}
              Maple AI
            </h2>
          }
          subtitle={
            <p className="text-2xl text-[hsl(var(--marketing-text-muted))] max-w-2xl mx-auto">
              Building AI with freedom of thought, privacy, and confidentiality.
            </p>
          }
        />

        <div className="flex flex-col gap-8 text-foreground pt-8 max-w-4xl mx-auto">
          {/* Maple Trees Section */}
          <div className="flex flex-col gap-6 dark:border-white/10 border-[hsl(var(--marketing-card-border))] dark:bg-black/75 bg-[hsl(var(--marketing-card))]/80 dark:text-white p-8 border rounded-lg">
            <h3 className="text-2xl font-medium">Our Inspiration</h3>
            <p className="text-lg leading-relaxed">
              Maple trees communicate securely underground, allowing them to share resources and
              adapt to harsh conditions. This is how Maple AI got its name. We need to have Freedom
              of Thought with AI that is built on privacy and confidentiality.
            </p>
          </div>

          {/* Founders Section */}
          <div className="flex flex-col gap-6 dark:border-white/10 border-[hsl(var(--marketing-card-border))] dark:bg-black/75 bg-[hsl(var(--marketing-card))]/80 dark:text-white p-8 border rounded-lg">
            <h3 className="text-2xl font-medium">Founders</h3>
            <div className="grid md:grid-cols-2 gap-8">
              {/* Founder 1 */}
              <div className="flex flex-col items-center text-center gap-4">
                <div className="w-32 h-32 bg-gradient-to-br from-[hsl(var(--purple))] to-[hsl(var(--blue))] rounded-full flex items-center justify-center">
                  <span className="text-white text-lg font-medium">Photo</span>
                </div>
                <div>
                  <h4 className="text-xl font-medium mb-1">[Founder Name]</h4>
                  <p className="text-[hsl(var(--marketing-text-muted))] mb-3">[Title]</p>
                  <p className="text-sm leading-relaxed max-w-xs">
                    [Placeholder for short description of 50 words about the founder's background,
                    expertise, and role in building Maple AI's vision for private and secure
                    artificial intelligence.]
                  </p>
                </div>
              </div>

              {/* Founder 2 */}
              <div className="flex flex-col items-center text-center gap-4">
                <div className="w-32 h-32 bg-gradient-to-br from-[hsl(var(--blue))] to-[hsl(var(--purple))] rounded-full flex items-center justify-center">
                  <span className="text-white text-lg font-medium">Photo</span>
                </div>
                <div>
                  <h4 className="text-xl font-medium mb-1">[Founder Name]</h4>
                  <p className="text-[hsl(var(--marketing-text-muted))] mb-3">[Title]</p>
                  <p className="text-sm leading-relaxed max-w-xs">
                    [Placeholder for short description of 50 words about the founder's background,
                    expertise, and role in building Maple AI's vision for private and secure
                    artificial intelligence.]
                  </p>
                </div>
              </div>
            </div>
          </div>

          {/* OpenSecret Company Section */}
          <div className="flex flex-col gap-6 dark:border-white/10 border-[hsl(var(--marketing-card-border))] dark:bg-black/75 bg-[hsl(var(--marketing-card))]/80 dark:text-white p-8 border rounded-lg">
            <h3 className="text-2xl font-medium">Built by OpenSecret</h3>
            <div className="flex flex-col md:flex-row items-center gap-6">
              <div className="w-24 h-24 bg-gradient-to-br from-[hsl(var(--purple))] to-[hsl(var(--blue))] rounded-lg flex items-center justify-center">
                <span className="text-white text-sm font-medium">Logo</span>
              </div>
              <div className="flex-1 text-center md:text-left">
                <p className="text-lg leading-relaxed mb-4">
                  Maple AI is built by OpenSecret, pioneering the future of privacy-preserving
                  artificial intelligence.
                </p>
                <a
                  href="https://opensecret.cloud"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="inline-flex items-center text-[hsl(var(--blue))] hover:text-[hsl(var(--purple))] transition-colors underline"
                >
                  Learn more about OpenSecret →
                </a>
              </div>
            </div>
          </div>

          {/* Developer Programs Section */}
          <div className="flex flex-col gap-6 dark:border-white/10 border-[hsl(var(--marketing-card-border))] dark:bg-black/75 bg-[hsl(var(--marketing-card))]/80 dark:text-white p-8 border rounded-lg">
            <h3 className="text-2xl font-medium">Developer Programs</h3>
            <div className="flex flex-wrap justify-center gap-8">
              {/* Nvidia Inception */}
              <div className="flex flex-col items-center gap-3">
                <div className="w-32 h-16 bg-gradient-to-br from-green-500 to-green-600 rounded-lg flex items-center justify-center">
                  <span className="text-white text-sm font-medium">NVIDIA</span>
                </div>
                <span className="text-sm text-[hsl(var(--marketing-text-muted))]">
                  Inception Program
                </span>
              </div>

              {/* Google Cloud */}
              <div className="flex flex-col items-center gap-3">
                <div className="w-32 h-16 bg-gradient-to-br from-blue-500 to-blue-600 rounded-lg flex items-center justify-center">
                  <span className="text-white text-sm font-medium">Google Cloud</span>
                </div>
                <span className="text-sm text-[hsl(var(--marketing-text-muted))]">
                  Developer Program
                </span>
              </div>
            </div>
          </div>

          {/* Built in Austin Footer */}
          <div className="text-center py-8">
            <p className="text-lg text-[hsl(var(--marketing-text-muted))]">
              Built in Austin, TX. Living in Secure Enclaves.
            </p>
          </div>
        </div>
      </FullPageMain>
    </>
  );
}
