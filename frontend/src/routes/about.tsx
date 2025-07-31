import { createFileRoute } from "@tanstack/react-router";
import { TopNav } from "@/components/TopNav";
import { FullPageMain } from "@/components/FullPageMain";
import { MarketingHeader } from "@/components/MarketingHeader";

export const Route = createFileRoute("/about")({
  component: About
});

function About(): JSX.Element {
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
              Incredibly powerful AI that doesn't share your data with anyone
            </p>
          }
        />

        <div className="flex flex-col gap-8 text-foreground pt-8 max-w-4xl mx-auto">
          {/* Maple Trees Section */}
          <div
            className={
              "flex flex-col gap-6 dark:border-white/10 " +
              "border-[hsl(var(--marketing-card-border))] dark:bg-black/75 " +
              "bg-[hsl(var(--marketing-card))]/80 dark:text-white p-8 border rounded-lg"
            }
          >
            <h3 className="text-2xl font-medium">Our Inspiration</h3>
            <div className="flex flex-col md:flex-row gap-8 items-start">
              <div className="flex flex-col items-center w-full md:w-48 flex-shrink-0">
                <img
                  src="/maple-autumn-forest.jpg"
                  alt="Maple autumn forest"
                  className="w-full md:w-48 h-auto rounded-lg object-contain md:object-cover"
                  style={{ maxHeight: '220px' }}
                />
                <span className="block text-xs text-[hsl(var(--marketing-text-muted))] mt-2 text-center">
                  Autumn Woods <br /> by William Trost Richards
                </span>
                <img
                  src="/maple-app-icon-vector.svg"
                  alt="Maple App Icon"
                  className="w-32 h-32 md:w-40 md:h-40 mt-4 rounded-lg object-contain hidden md:block"
                />
              </div>
              <div className="flex flex-col gap-4 flex-1">
                <p className="text-lg leading-relaxed">
                Maple trees form underground networks of root systems, 
                communicating and sharing resources without the knowledge of animals above. 
                This cooperative approach helps them thrive in challenging environments.
                </p>
                <p className="text-lg leading-relaxed">
                  Maple AI draws inspiration from this natural model of secure, decentralized intelligence. 
                  Each account uses a personal encryption key, giving you control over the data. 
                  Whether you are handling sensitive information on behalf of clients or your own personal thoughts, 
                  communication stays between you and the AI. No data is used for training models nor shared with third parties, 
                  not even us.
                </p>
                <p className="text-lg leading-relaxed">
                  People shouldn't have to sacrifice their privacy for high-quality AI.
                  Maple upholds the highest standards of privacy and confidentiality, 
                  preserving the fundamental human right to freedom of thought.
                  With confidence in AI, you can work together to thrive in your own challenging environments.
                </p>
                <img
                  src="/maple-app-icon-vector.svg"
                  alt="Maple App Icon"
                  className="w-32 h-32 mt-4 rounded-lg object-contain block md:hidden mx-auto"
                />
              </div>
            </div>
          </div>

          {/* Founders Section */}
          <div
            className={
              "flex flex-col gap-6 dark:border-white/10 " +
              "border-[hsl(var(--marketing-card-border))] dark:bg-black/75 " +
              "bg-[hsl(var(--marketing-card))]/80 dark:text-white p-8 border rounded-lg"
            }
          >
            <h3 className="text-2xl font-medium">Founders</h3>
            <p className="text-lg leading-relaxed">
              Our team has combined over 30 years experience building scalable cloud and local app solutions
              that take privacy and security seriously. We have successfully built in the fields of data storage, 
              defense, fintech, education, security, therapy, computer vision, and consumer technology.
            </p>
            <div className="grid md:grid-cols-2 gap-8">
              {/* Founder 1 */}
              <div className="flex flex-col items-center text-center gap-4">
                <a href="https://x.com/marks_ftw" target="_blank" rel="noopener noreferrer" aria-label="Mark Suman on X">
                  <img
                    src="/mark.jpg"
                    alt="Mark, Co-founder of Maple AI"
                    className="w-32 h-32 rounded-full object-cover border-4 border-[hsl(var(--purple))]"
                  />
                </a>
                <div>
                  <h4 className="text-xl font-medium mb-1">Mark Suman</h4>
                  <p className="text-[hsl(var(--marketing-text-muted))] mb-3">CEO</p>
                  <p className="text-sm leading-relaxed max-w-xs">
                    Early employee in Product and Engineering at multiple startups, including Instructure Canvas. Most recently, 6 years in
                    software engineering at Apple with a focus on AI and Privacy.
                    <span className="inline-flex items-center align-middle ml-2 gap-1">
                      <a
                        href="https://x.com/marks_ftw"
                        target="_blank"
                        rel="noopener noreferrer"
                        className="inline-flex items-center justify-center w-5 h-5"
                        aria-label="Mark Suman on X"
                      >
                        <svg viewBox="0 0 24 24" className="w-4 h-4 align-middle">
                          <rect x="4" y="4" width="16" height="16" rx="2"
                            className="fill-[hsl(var(--purple))] stroke-[hsl(var(--purple))] dark:fill-transparent dark:stroke-[hsl(var(--purple))]"
                            fill="currentColor" stroke="currentColor" strokeWidth="2"/>
                          <path d="M8 8l8 8M16 8l-8 8"
                            className="stroke-white dark:stroke-[hsl(var(--purple))]"
                            stroke="currentColor" strokeWidth="2" strokeLinecap="round"/>
                        </svg>
                      </a>
                      <a
                        href="https://linkedin.com/in/marksuman"
                        target="_blank"
                        rel="noopener noreferrer"
                        className="inline-flex items-center justify-center w-5 h-5"
                        aria-label="Mark Suman on LinkedIn"
                      >
                        <svg viewBox="0 0 24 24" className="w-4 h-4 align-middle">
                          <rect x="4" y="4" width="16" height="16" rx="2"
                            className="fill-[hsl(var(--purple))] stroke-[hsl(var(--purple))] dark:fill-transparent dark:stroke-[hsl(var(--purple))]"
                            fill="currentColor" stroke="currentColor" strokeWidth="2"/>
                          <path d="M8 11v5M8 8v.01M12 16v-5m0 0a2 2 0 1 1 4 0v5"
                            className="stroke-white dark:stroke-[hsl(var(--purple))]"
                            stroke="currentColor" strokeWidth="2" strokeLinecap="round"/>
                        </svg>
                      </a>
                    </span>
                  </p>
                </div>
              </div>

              {/* Founder 2 */}
              <div className="flex flex-col items-center text-center gap-4">
                <a href="https://x.com/anthonyronning_" target="_blank" rel="noopener noreferrer" aria-label="Anthony Ronning on X">
                  <img
                    src="/anthony.jpg"
                    alt="Anthony, Co-founder of Maple AI"
                    className="w-32 h-32 rounded-full object-cover border-4 border-[hsl(var(--blue))]"
                  />
                </a>
                <div>
                  <h4 className="text-xl font-medium mb-1">Anthony Ronning</h4>
                  <p className="text-[hsl(var(--marketing-text-muted))] mb-3">CTO</p>
                  <p className="text-sm leading-relaxed max-w-xs">
                    Infrastructure engineer in many startups over the last 9 years. Previous
                    experience in defense, security, networking, and bitcoin companies.
                    <span className="inline-flex items-center align-middle ml-2 gap-1">
                      <a
                        href="https://x.com/anthonyronning_"
                        target="_blank"
                        rel="noopener noreferrer"
                        className="inline-flex items-center justify-center w-5 h-5"
                        aria-label="Anthony Ronning on X"
                      >
                        <svg viewBox="0 0 24 24" className="w-4 h-4 align-middle">
                          <rect x="4" y="4" width="16" height="16" rx="2"
                            className="fill-[hsl(var(--blue))] stroke-[hsl(var(--blue))] dark:fill-transparent dark:stroke-[hsl(var(--blue))]"
                            fill="currentColor" stroke="currentColor" strokeWidth="2"/>
                          <path d="M8 8l8 8M16 8l-8 8"
                            className="stroke-white dark:stroke-[hsl(var(--blue))]"
                            stroke="currentColor" strokeWidth="2" strokeLinecap="round"/>
                        </svg>
                      </a>
                    </span>
                  </p>
                </div>
              </div>
            </div>
          </div>

          {/* OpenSecret Company Section */}
          <div
            className={
              "flex flex-col gap-6 dark:border-white/10 " +
              "border-[hsl(var(--marketing-card-border))] dark:bg-black/75 " +
              "bg-[hsl(var(--marketing-card))]/80 dark:text-white p-8 border rounded-lg"
            }
          >
            <h3 className="text-2xl font-medium">Built by OpenSecret</h3>
            <div className="flex flex-col md:flex-row items-center gap-6">
              <img
                src="/opensecret-logo.png"
                alt="OpenSecret company logo"
                className="w-24 h-24 rounded-lg object-cover"
                style={{ objectPosition: "center" }}
              />
              <div className="flex-1 text-center md:text-left">
                <p className="text-lg leading-relaxed mb-4">
                  Maple AI is built by OpenSecret, pioneering the future of privacy-preserving apps.
                  Whether artificial intelligence or traditional apps, user data is end-to-end
                  encrypted.
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
          <div
            className={
              "flex flex-col gap-6 dark:border-white/10 " +
              "border-[hsl(var(--marketing-card-border))] dark:bg-black/75 " +
              "bg-[hsl(var(--marketing-card))]/80 dark:text-white p-8 border rounded-lg"
            }
          >
            <h3 className="text-2xl font-medium">Developer Programs</h3>
            <div className="flex flex-wrap justify-center gap-8">
              {/* Nvidia Inception */}
              <div className="flex flex-col items-center gap-3">
                <img
                  src="/nvidia-inception.png"
                  alt="NVIDIA Inception Program logo"
                  className="w-32 h-16 object-contain"
                />
                <span className="text-sm text-[hsl(var(--marketing-text-muted))]">
                  Inception Program
                </span>
              </div>

              {/* Google Cloud */}
              <div className="flex flex-col items-center gap-3">
                <img
                  src="/google-cloud-for-startups.png"
                  alt="Google Cloud for Startups program logo"
                  className="w-32 h-16 object-contain"
                />
                <span className="text-sm text-[hsl(var(--marketing-text-muted))]">
                  Google Cloud for Startups
                </span>
              </div>
            </div>
          </div>

          {/* Built in Austin Footer */}
          <div className="text-center py-8">
            <p className="text-lg text-[hsl(var(--marketing-text-muted))]">
              Built in Austin. Living in Secure Enclaves.
            </p>
          </div>
        </div>
      </FullPageMain>
    </>
  );
}
