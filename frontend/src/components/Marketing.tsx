import { Link } from "@tanstack/react-router";
import { VerificationStatus } from "./VerificationStatus";
import { MessageSquareMore } from "lucide-react";
import { MarketingHeader } from "./MarketingHeader";
import { Footer } from "./Footer";

function CTAButton({
  children,
  to,
  primary = false
}: {
  children: React.ReactNode;
  to: string;
  primary?: boolean;
}) {
  return (
    <Link
      to={to}
      className={`
                flex items-center justify-center
                px-8 py-4 rounded-lg text-xl font-light
                transition-all duration-200
                shadow-[0_0_25px_rgba(255,255,255,0.25)]
                ${
                  primary
                    ? "bg-white/90 backdrop-blur-sm text-black hover:bg-white/70 active:bg-white/80 hover:shadow-[0_0_35px_rgba(255,255,255,0.35)]"
                    : "bg-black/75 backdrop-blur-sm hover:bg-black/60 active:bg-black/75 hover:shadow-[0_0_35px_rgba(255,255,255,0.15)]"
                }
            `}
    >
      {children}
    </Link>
  );
}

export function Marketing() {
  return (
    <div className="flex flex-col items-center gap-12 text-foreground pt-24 text-white">
      <div className="flex flex-col items-center gap-8">
        <a
          href="http://blog.opensecret.cloud/maple-private-ai-for-work-and-personal"
          target="_blank"
          rel="noopener noreferrer"
          className="bg-white/10 backdrop-blur-sm px-4 py-1.5 rounded-full border border-white/20 text-sm font-light hover:bg-white/20 transition-colors"
        >
          🎉 Now Live • Read the Announcement →
        </a>
        <MarketingHeader
          title="Private AI Chat"
          className="pt-0"
          subtitle={
            <>
              Encrypted. At every step.
              <br />
              Nobody can read your chats but you.
            </>
          }
        />
      </div>
      <div className="flex gap-6 flex-col sm:flex-row items-center">
        <CTAButton to="/signup" primary>
          <MessageSquareMore className="h-6 w-6 mr-2" />
          Start Chatting Securely
        </CTAButton>
        <CTAButton to="/login">Log In</CTAButton>
      </div>
      <div className="w-full grid grid-cols-1 md:grid-cols-3 gap-4 md:gap-8">
        <div className="flex flex-col gap-2 py-4 border-white/10 bg-black/75 text-whit p-8 border rounded-lg">
          <img src="/device-gradient.svg" alt="mask test" className="w-32 h-32 self-center p-4" />
          <h3 className="text-2xl font-medium">Your Device</h3>
          <p className="text-lg font-light text-white/70">
            All your communications are encrypted locally before being sent to our servers.
          </p>
        </div>
        <div className="flex flex-col gap-2 py-4 border-white/10 bg-black/75 text-whit p-8 border rounded-lg">
          <img src="/server-gradient.svg" alt="mask test" className="w-32 h-32 self-center p-4" />
          <h3 className="text-2xl font-medium">Secure Server</h3>
          <p className="text-lg font-light text-white/70">
            Our servers can't be spied on, even by us. And we can prove it.
          </p>
          <VerificationStatus />
        </div>
        <div className="flex flex-col gap-2 py-4 border-white/10 bg-black/75 text-whit p-8 border rounded-lg">
          <img src="/cloud-gradient.svg" alt="mask test" className="w-32 h-32 self-center p-4" />
          <h3 className="text-2xl font-medium">AI Cloud</h3>
          <p className="text-lg font-light text-white/70">
            Your chats are encrypted and sent to a GPU, providing a highly secure transmission
            that's resistant to interception.
          </p>
        </div>
      </div>
      <Footer />
    </div>
  );
}
