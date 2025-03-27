import { Link } from "@tanstack/react-router";
import { VerificationStatus } from "./VerificationStatus";
import { ArrowRight, Check, Lock, MessageSquareMore, Shield, Sparkles } from "lucide-react";
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
        flex items-center justify-center gap-2
        px-8 py-4 rounded-lg text-xl font-light
        transition-all duration-300 relative
        ${
          primary
            ? "bg-[#9469F8] text-[#111111] hover:bg-[#A57FF9] shadow-[0_0_20px_rgba(148,105,248,0.3)]"
            : "bg-[#111111] border border-[#3FDBFF]/20 text-[#E2E2E2] hover:border-[#3FDBFF]/80 shadow-[0_0_15px_rgba(63,219,255,0.1)]"
        }
      `}
    >
      {children}
      {primary && (
        <div className="absolute inset-0 rounded-lg overflow-hidden">
          <div
            className="absolute inset-0 bg-gradient-to-r from-[#9469F8]/0 via-[#3FDBFF]/20 to-[#9469F8]/0 opacity-50 animate-shimmer"
            style={{ transform: "translateX(-100%)" }}
          ></div>
        </div>
      )}
    </Link>
  );
}

function FeatureCard({
  icon: Icon,
  title,
  description,
  gradient = "from-[#9469F8]/10 to-[#3FDBFF]/10"
}: {
  icon: React.ElementType;
  title: string;
  description: string;
  gradient?: string;
}) {
  return (
    <div
      className={`flex flex-col gap-4 p-8 rounded-xl bg-gradient-to-br ${gradient} border border-[#E2E2E2]/10 hover:border-[#E2E2E2]/20 transition-all duration-300`}
    >
      <div className="p-3 rounded-full bg-[#111111]/50 border border-[#9469F8]/30 w-fit">
        <Icon className="w-6 h-6 text-[#9469F8]" />
      </div>
      <h3 className="text-2xl font-medium text-[#E2E2E2]">{title}</h3>
      <p className="text-lg font-light text-[#E2E2E2]/70">{description}</p>
    </div>
  );
}

function TestimonialCard({
  quote,
  author,
  role,
  avatar
}: {
  quote: string;
  author: string;
  role: string;
  avatar: string;
}) {
  return (
    <div className="p-6 rounded-xl bg-[#111111]/80 border border-[#E2E2E2]/10 flex flex-col gap-4">
      <p className="text-lg italic text-[#E2E2E2]/90 font-light">&ldquo;{quote}&rdquo;</p>
      <div className="flex items-center gap-3 mt-2">
        <img src={avatar} alt={author} className="w-12 h-12 rounded-full" />
        <div>
          <p className="font-medium text-[#E2E2E2]">{author}</p>
          <p className="text-sm text-[#E2E2E2]/60">{role}</p>
        </div>
      </div>
    </div>
  );
}

function PricingTier({
  name,
  price,
  description,
  features,
  ctaText,
  popular = false
}: {
  name: string;
  price: string;
  description: string;
  features: string[];
  ctaText: string;
  popular?: boolean;
}) {
  return (
    <div
      className={`flex flex-col p-8 rounded-xl ${popular ? "border-2 border-[#9469F8] bg-gradient-to-b from-[#111111] to-[#111111]/80 relative shadow-[0_0_30px_rgba(148,105,248,0.2)]" : "border border-[#E2E2E2]/10 bg-[#111111]/50"}`}
    >
      {popular && (
        <div className="absolute -top-4 left-1/2 transform -translate-x-1/2 bg-[#9469F8] text-[#111111] px-4 py-1 rounded-full text-sm font-medium">
          Most Popular
        </div>
      )}
      <h3 className="text-xl font-medium text-[#E2E2E2]">{name}</h3>
      <div className="mt-4 mb-2">
        <span className="text-4xl font-bold text-[#E2E2E2]">{price}</span>
        {price !== "Free" && <span className="text-[#E2E2E2]/60 ml-2">/mo</span>}
      </div>
      <p className="text-[#E2E2E2]/70 mb-6">{description}</p>
      <div className="flex flex-col gap-3 mb-8">
        {features.map((feature, index) => (
          <div key={index} className="flex items-start gap-2">
            <Check className="w-5 h-5 text-[#A1FE8F] mt-0.5 flex-shrink-0" />
            <span className="text-[#E2E2E2]/80">{feature}</span>
          </div>
        ))}
      </div>
      <Link
        to="/signup"
        className={`mt-auto py-3 px-6 rounded-lg text-center font-medium transition-all duration-300 ${
          popular
            ? "bg-[#9469F8] text-[#111111] hover:bg-[#A57FF9]"
            : "bg-[#111111] border border-[#E2E2E2]/20 text-[#E2E2E2] hover:border-[#E2E2E2]/40"
        }`}
      >
        {ctaText}
      </Link>
    </div>
  );
}

export function Marketing() {
  return (
    <div className="flex flex-col items-center w-full text-[#E2E2E2]">
      {/* Hero Section */}
      <section className="w-full max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 pt-32 pb-24">
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-16 items-center">
          <div className="flex flex-col gap-8">
            <div className="flex flex-col gap-2">
              <h1 className="text-6xl font-light tracking-tight mb-4 bg-gradient-to-r from-[#E2E2E2] via-[#3FDBFF] to-[#9469F8] text-transparent bg-clip-text">
                Private AI Chat <br /> <span className="text-[#A1FE8F]">that's truly secure</span>
              </h1>
              <p className="text-xl text-[#E2E2E2]/80 font-light max-w-xl">
                End-to-end encryption means your conversations are confidential and protected at every step. Only you
                can access your data — not even we can read your chats.
              </p>
            </div>
            <div className="flex gap-4 flex-col sm:flex-row">
              <CTAButton to="/signup" primary>
                <Sparkles className="h-5 w-5" />
                Start Secure Chat
              </CTAButton>
              <CTAButton to="/login">Log In</CTAButton>
            </div>
            <div className="flex items-center gap-4 text-sm text-[#E2E2E2]/60 mt-2">
              <a 
                href="#testimonials" 
                className="flex items-center gap-4 hover:text-[#E2E2E2]/80 transition-colors duration-300"
                onClick={(e) => {
                  e.preventDefault();
                  document.getElementById('testimonials')?.scrollIntoView({ behavior: 'smooth' });
                }}
              >
                <div className="flex -space-x-2">
                  <img
                    src="/ryan-g.jpg"
                    alt="User avatar"
                    className="w-8 h-8 rounded-full border border-[#111111]"
                  />
                  <img
                    src="/lauren-t.jpg"
                    alt="User avatar"
                    className="w-8 h-8 rounded-full border border-[#111111]"
                  />
                  <img
                    src="/javier-r.jpg"
                    alt="User avatar"
                    className="w-8 h-8 rounded-full border border-[#111111]"
                  />
                </div>
                <p className="ml-10 md:ml-0">Trusted by professionals who handle sensitive client information</p>
              </a>
            </div>
          </div>
          <div className="relative bg-gradient-to-br from-[#9469F8]/10 to-[#3FDBFF]/10 rounded-2xl p-1">
            <div className="bg-[#111111]/80 backdrop-blur-sm rounded-xl overflow-hidden border border-[#E2E2E2]/10">
              <div className="bg-[#111111] p-3 border-b border-[#E2E2E2]/10 flex items-center">
                <div className="flex gap-1.5">
                  <div className="w-3 h-3 rounded-full bg-[#E2E2E2]/20"></div>
                  <div className="w-3 h-3 rounded-full bg-[#E2E2E2]/20"></div>
                  <div className="w-3 h-3 rounded-full bg-[#E2E2E2]/20"></div>
                </div>
                <div className="mx-auto text-sm text-[#E2E2E2]/60">Encrypted Chat</div>
              </div>
              <div className="p-6 space-y-4">
                <div className="flex gap-3">
                  <div className="w-8 h-8 rounded-full bg-[#9469F8]/30 flex items-center justify-center text-[#9469F8] flex-shrink-0">
                    AI
                  </div>
                  <div className="bg-[#1D1D1D] p-3 rounded-xl rounded-tl-none text-[#E2E2E2]/90 text-sm max-w-xs">
                    How can I help you today? Your conversation is fully encrypted.
                  </div>
                </div>
                <div className="flex gap-3 justify-end">
                  <div className="bg-[#9469F8]/20 p-3 rounded-xl rounded-tr-none text-[#E2E2E2] text-sm max-w-xs">
                    I need to discuss some sensitive information. Is this really secure?
                  </div>
                  <div className="w-8 h-8 rounded-full bg-[#E2E2E2]/10 flex items-center justify-center text-[#E2E2E2] flex-shrink-0">
                    U
                  </div>
                </div>
                <div className="flex gap-3">
                  <div className="w-8 h-8 rounded-full bg-[#9469F8]/30 flex items-center justify-center text-[#9469F8] flex-shrink-0">
                    AI
                  </div>
                  <div className="bg-[#1D1D1D] p-3 rounded-xl rounded-tl-none text-[#E2E2E2]/90 text-sm max-w-xs">
                    Yes, absolutely. Your messages are end-to-end encrypted. Even the server admins
                    can't read your conversations. We use secure enclaves and open-source code to verify this.
                  </div>
                </div>
              </div>
            </div>
            <div className="absolute -bottom-3 -right-3 bg-[#9469F8] text-[#111111] p-2 rounded-lg font-medium text-sm flex items-center gap-1.5">
              <Lock className="w-4 h-4" /> End-to-End Encrypted
            </div>
          </div>
        </div>

        {/* Download Buttons */}
        <div className="flex flex-col md:flex-row justify-center items-center gap-4 mt-20">
          <a 
            href="#" 
            className="transition-opacity hover:opacity-80"
            target="_blank"
            rel="noopener noreferrer"
          >
            <img 
              src="/app-store-badge.svg" 
              alt="Download on the App Store" 
              className="h-[40px] w-auto"
            />
          </a>
          <a 
            href="#" 
            className="transition-opacity hover:opacity-80"
            target="_blank"
            rel="noopener noreferrer"
          >
            <img 
              src="/google-play-badge.png" 
              alt="Get it on Google Play" 
              className="h-[40px] w-auto"
            />
          </a>
          <a 
            href="#" 
            className="flex items-center gap-2 h-[40px] px-2 bg-[#111111] rounded-md border border-[#FFFFFF]/45 transition-opacity hover:opacity-80"
            target="_blank"
            rel="noopener noreferrer"
          >
            <svg 
              className="w-5 h-5 text-[#FFFFFF]" 
              viewBox="0 0 24 24" 
              fill="none" 
              stroke="currentColor" 
              strokeWidth="2"
            >
              <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4" />
              <polyline points="7 10 12 15 17 10" />
              <line x1="12" y1="15" x2="12" y2="3" />
            </svg>
            <div className="flex flex-col text-left leading-[1.1]">
              <span className="text-[10px] text-[#FFFFFF]">Download for</span>
              <span className="text-[15px] font-medium text-[#FFFFFF]">macOS</span>
            </div>
          </a>
        </div>
      </section>

      {/* Features Section */}
      <section className="w-full py-20 bg-[#13131A]">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="text-center mb-16">
            <h2 className="text-4xl font-light mb-4">
              Privacy at <span className="text-[#9469F8]">Every Layer</span>
            </h2>
            <p className="text-xl text-[#E2E2E2]/70 max-w-2xl mx-auto">
              Your security is our primary concern. We've engineered Maple to ensure your data
              remains yours alone, protected by cutting-edge encryption.
            </p>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-8">
            <FeatureCard
              icon={Shield}
              title="Your Device"
              description="All communications are encrypted locally on your device before being transmitted, ensuring your data is secured from the start."
              gradient="from-[#9469F8]/10 to-[#9469F8]/5"
            />
            <FeatureCard
              icon={Lock}
              title="Secure Server"
              description="Our servers can't read your data. We use secure enclaves in confidential computing environments to verify our infrastructure integrity."
              gradient="from-[#3FDBFF]/10 to-[#3FDBFF]/5"
            />
            <FeatureCard
              icon={Sparkles}
              title="AI Processing"
              description="Even during AI processing, your data remains encrypted. The entire pipeline through to the GPU is designed with privacy as the priority."
              gradient="from-[#A1FE8F]/10 to-[#A1FE8F]/5"
            />
          </div>
        </div>
      </section>

      {/* Proof/Verification Section */}
      <section className="w-full pt-20 pb-0">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="grid grid-cols-1 lg:grid-cols-2 gap-16 items-center">
            <div>
              <h2 className="text-4xl font-light mb-6">
                We <span className="text-[#3FDBFF]">Prove</span> Our Security
              </h2>
              <p className="text-xl text-[#E2E2E2]/70 mb-8">
                Unlike other services that merely claim to be secure, we provide cryptographic
                proof. Our commitment to transparency means you don't have to trust us - you can
                verify it yourself.
              </p>
              <div className="text-l text-[#E2E2E2]/70">Live Secure Enclave Verification</div>
              <div className="bg-[#111111]/80 border border-[#E2E2E2]/10 rounded-xl p-6 mb-8">
                <VerificationStatus />
              </div>
              <Link
                to="/proof"
                className="text-[#3FDBFF] hover:text-[#3FDBFF]/80 flex items-center gap-2 font-medium"
              >
                Learn more about our verification system <ArrowRight className="h-4 w-4" />
              </Link>
            </div>
            <div className="relative bg-gradient-to-br from-[#9469F8]/10 to-[#3FDBFF]/10 rounded-2xl p-1">
              <div className="bg-[#111111]/80 backdrop-blur-sm rounded-xl overflow-hidden border border-[#E2E2E2]/10">
                {/* Header */}
                <div className="bg-[#111111] p-3 border-b border-[#E2E2E2]/10 flex items-center">
                  {/* <div className="flex gap-1.5">
                    <div className="w-3 h-3 rounded-full bg-[#E2E2E2]/20"></div>
                    <div className="w-3 h-3 rounded-full bg-[#E2E2E2]/20"></div>
                    <div className="w-3 h-3 rounded-full bg-[#E2E2E2]/20"></div>
                  </div> */}
                  <div className="mx-auto text-sm text-[#E2E2E2]/60">Secure Server</div>
                </div>
                
                {/* Main Content */}
                <div className="p-8 relative">
                  <div className="w-full mx-auto relative flex flex-col md:flex-row items-center justify-between px-2">
                    {/* Container for mobile view to ensure proper spacing */}
                    <div className="flex flex-col md:hidden items-center gap-8 w-full">
                      {/* Phone - Mobile */}
                      <div className="w-24 h-48 relative">
                        <div className="absolute inset-0 bg-[#1D1D1D] border border-[#9469F8]/30 rounded-2xl">
                          <div className="m-2 bg-[#9469F8]/10 rounded-lg border border-[#9469F8]/30 h-[calc(100%-16px)]">
                            <div className="h-2 w-8 bg-[#9469F8]/20 rounded mx-auto mt-2"></div>
                            <div className="h-2 w-12 bg-[#9469F8]/20 rounded mx-auto mt-2"></div>
                          </div>
                        </div>
                        <div className="absolute -top-2 -right-2 bg-[#3FDBFF]/20 p-2 rounded-lg border border-[#3FDBFF]/30">
                          <div className="w-4 h-3 border-2 border-[#3FDBFF] rounded-t-lg mx-auto"></div>
                          <div className="w-6 h-5 bg-[#3FDBFF]/30 border-2 border-[#3FDBFF] rounded-lg -mt-0.5"></div>
                        </div>
                      </div>

                      {/* Vertical Connection Line - Mobile */}
                      <div className="h-16 w-px bg-gradient-to-b from-[#9469F8]/50 to-[#3FDBFF]/50 relative">
                        <div className="absolute -top-1 left-1/2 -translate-x-1/2 w-2 h-2 border-t-2 border-l-2 border-[#9469F8] transform rotate-45"></div>
                        <div className="absolute -bottom-1 left-1/2 -translate-x-1/2 w-2 h-2 border-b-2 border-r-2 border-[#3FDBFF] transform rotate-45"></div>
                        <div className="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 whitespace-nowrap text-xs text-[#E2E2E2]/40 ml-1">
                          Encrypted Connection
                        </div>
                      </div>

                      {/* Server - Mobile */}
                      <div className="w-32 h-48 relative">
                        <div className="absolute inset-0 bg-[#1D1D1D] border border-[#9469F8]/30 rounded-lg">
                          <div className="h-8 border-b border-[#9469F8]/30 flex items-center justify-between px-3">
                            <div className="flex gap-1">
                              <div className="w-2 h-2 rounded-full bg-[#9469F8]/40"></div>
                              <div className="w-2 h-2 rounded-full bg-[#9469F8]/40"></div>
                            </div>
                          </div>
                          <div className="p-3 space-y-2">
                            <div className="h-7 bg-[#9469F8]/20 rounded border border-[#9469F8]/30 flex items-center px-2">
                              <div className="w-2 h-2 rounded-full bg-[#9469F8]/40"></div>
                            </div>
                            <div className="h-7 bg-[#9469F8]/20 rounded border border-[#9469F8]/30 flex items-center px-2">
                              <div className="w-2 h-2 rounded-full bg-[#9469F8]/40"></div>
                            </div>
                            <div className="h-7 bg-[#9469F8]/20 rounded border border-[#9469F8]/30 flex items-center px-2">
                              <div className="w-2 h-2 rounded-full bg-[#9469F8]/40"></div>
                            </div>
                          </div>
                        </div>
                        <div className="absolute -top-2 -right-2 bg-[#3FDBFF]/20 p-2 rounded-lg border border-[#3FDBFF]/30">
                          <div className="w-4 h-3 border-2 border-[#3FDBFF] rounded-t-lg mx-auto"></div>
                          <div className="w-6 h-5 bg-[#3FDBFF]/30 border-2 border-[#3FDBFF] rounded-lg -mt-0.5"></div>
                        </div>
                      </div>
                    </div>

                    {/* Desktop layout - hidden on mobile */}
                    <div className="hidden md:flex items-center justify-between w-full">
                      {/* Phone - Desktop */}
                      <div className="w-24 h-48 relative">
                        {/* Same phone content as mobile */}
                        <div className="absolute inset-0 bg-[#1D1D1D] border border-[#9469F8]/30 rounded-2xl">
                          <div className="m-2 bg-[#9469F8]/10 rounded-lg border border-[#9469F8]/30 h-[calc(100%-16px)]">
                            <div className="h-2 w-8 bg-[#9469F8]/20 rounded mx-auto mt-2"></div>
                            <div className="h-2 w-12 bg-[#9469F8]/20 rounded mx-auto mt-2"></div>
                          </div>
                        </div>
                        <div className="absolute -top-2 -right-2 bg-[#3FDBFF]/20 p-2 rounded-lg border border-[#3FDBFF]/30">
                          <div className="w-4 h-3 border-2 border-[#3FDBFF] rounded-t-lg mx-auto"></div>
                          <div className="w-6 h-5 bg-[#3FDBFF]/30 border-2 border-[#3FDBFF] rounded-lg -mt-0.5"></div>
                        </div>
                      </div>

                      {/* Horizontal Connection Line - Desktop */}
                      <div className="flex-1 mx-2 flex items-center justify-center">
                        <div className="h-px w-4/5 bg-gradient-to-r from-[#9469F8]/50 to-[#3FDBFF]/50 relative">
                          <div className="absolute left-0 top-1/2 -translate-y-1/2 w-2 h-2 border-t-2 border-l-2 border-[#9469F8] transform -rotate-45"></div>
                          <div className="absolute right-0 top-1/2 -translate-y-1/2 w-2 h-2 border-t-2 border-r-2 border-[#3FDBFF] transform rotate-45"></div>
                          <div className="absolute -top-4 left-1/2 -translate-x-1/2 whitespace-nowrap text-xs text-[#E2E2E2]/40">
                            Encrypted Connection
                          </div>
                        </div>
                      </div>

                      {/* Server - Desktop */}
                      <div className="w-32 h-48 relative">
                        {/* Same server content as mobile */}
                        <div className="absolute inset-0 bg-[#1D1D1D] border border-[#9469F8]/30 rounded-lg">
                          <div className="h-8 border-b border-[#9469F8]/30 flex items-center justify-between px-3">
                            <div className="flex gap-1">
                              <div className="w-2 h-2 rounded-full bg-[#9469F8]/40"></div>
                              <div className="w-2 h-2 rounded-full bg-[#9469F8]/40"></div>
                            </div>
                          </div>
                          <div className="p-3 space-y-2">
                            <div className="h-7 bg-[#9469F8]/20 rounded border border-[#9469F8]/30 flex items-center px-2">
                              <div className="w-2 h-2 rounded-full bg-[#9469F8]/40"></div>
                            </div>
                            <div className="h-7 bg-[#9469F8]/20 rounded border border-[#9469F8]/30 flex items-center px-2">
                              <div className="w-2 h-2 rounded-full bg-[#9469F8]/40"></div>
                            </div>
                            <div className="h-7 bg-[#9469F8]/20 rounded border border-[#9469F8]/30 flex items-center px-2">
                              <div className="w-2 h-2 rounded-full bg-[#9469F8]/40"></div>
                            </div>
                          </div>
                        </div>
                        <div className="absolute -top-2 -right-2 bg-[#3FDBFF]/20 p-2 rounded-lg border border-[#3FDBFF]/30">
                          <div className="w-4 h-3 border-2 border-[#3FDBFF] rounded-t-lg mx-auto"></div>
                          <div className="w-6 h-5 bg-[#3FDBFF]/30 border-2 border-[#3FDBFF] rounded-lg -mt-0.5"></div>
                        </div>
                      </div>
                    </div>
                  </div>
                  
                  {/* Crypto Text */}
                  <div className="mt-8 flex gap-4 justify-center overflow-hidden text-xs font-mono">
                    <div className="text-[#3FDBFF]/60">0x7B4...</div>
                    <div className="text-[#9469F8]/60">RSA-2048</div>
                    <div className="text-[#A1FE8F]/60">AES-256</div>
                  </div>
                </div>
              </div>
              
              {/* Status Indicator */}
              <div className="absolute -bottom-3 -right-3 bg-[#9469F8] text-[#111111] p-2 rounded-lg font-medium text-sm flex items-center gap-1.5">
                <Shield className="w-4 h-4" /> Secure Enclave Verified
              </div>
            </div>
          </div>
        </div>
      </section>

      {/* Testimonials */}
      <section id="testimonials" className="w-full pt-40 pb-20 bg-[#13131A]">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="text-center mb-16">
            <h2 className="text-4xl font-light mb-4">
              What Our <span className="text-[#A1FE8F]">Users Say</span>
            </h2>
            <p className="text-xl text-[#E2E2E2]/70 max-w-2xl mx-auto">
              Hear from those who've made privacy a priority with Maple AI.
              <br />
              (Quotes are real, names are changed for privacy)
            </p>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-8">
            <TestimonialCard
              quote="For me, knowing it's end-to-end encrypted reassures me that proprietary information, client information, or any IP won't be exposed."
              author="Ryan G."
              role="VP Data & Analytics"
              avatar="/ryan-g.jpg"
            />
            <TestimonialCard
              quote="I've been using Maple AI to help my clients with their tax and financial needs. The encryption means that their sensitive financial data and situations stay private. It really gives me peace of mind."
              author="Lauren T."
              role="Certified Public Accountant"
              avatar="/lauren-t.jpg"
            />
            <TestimonialCard
              quote="I am fascinated by the possibilities of AI in the legal field. Finding Maple and knowing confidential client information is encrypted has been a game changer for our team."
              author="Javier R."
              role="Attorney"
              avatar="/javier-r.jpg"
            />
          </div>
        </div>
      </section>

      {/* Pricing Section */}
      <section className="w-full py-20">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="text-center mb-16">
            <h2 className="text-4xl font-light mb-4">
              Simple, <span className="text-[#9469F8]">Transparent</span> Pricing
            </h2>
            <p className="text-xl text-[#E2E2E2]/70 max-w-2xl mx-auto">
              No hidden fees. Choose the plan that works for your needs.
            </p>
          </div>

          <div className="grid grid-cols-1 md:grid-cols-3 gap-8">
            <PricingTier
              name="Basic"
              price="Free"
              description="Get started with secure AI chat"
              features={[
                "End-to-end encryption",
                "10 messages per day",
                "Access to core AI features",
                "7-day message history"
              ]}
              ctaText="Get Started"
            />
            <PricingTier
              name="Pro"
              price="$15"
              description="For power users who need more"
              features={[
                "Everything in Basic",
                "Unlimited messages",
                "Advanced AI models",
                "30-day message history",
                "Priority support"
              ]}
              ctaText="Try Pro"
              popular={true}
            />
            <PricingTier
              name="Enterprise"
              price="$49"
              description="For teams and businesses"
              features={[
                "Everything in Pro",
                "Team collaboration",
                "Custom security policies",
                "Unlimited message history",
                "Dedicated account manager",
                "Compliance reporting"
              ]}
              ctaText="Contact Sales"
            />
          </div>
        </div>
      </section>

      {/* CTA Section */}
      <section className="w-full py-20 bg-gradient-to-r from-[#111111] via-[#13131A] to-[#111111]">
        <div className="max-w-5xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="bg-gradient-to-br from-[#9469F8]/20 to-[#3FDBFF]/20 p-1 rounded-2xl">
            <div className="bg-[#111111]/95 rounded-2xl p-12 text-center">
              <h2 className="text-4xl font-light mb-4">
                Ready to Chat <span className="text-[#A1FE8F]">Securely?</span>
              </h2>
              <p className="text-xl text-[#E2E2E2]/70 max-w-2xl mx-auto mb-8">
                Join thousands of privacy-conscious users who've made the switch to truly secure AI
                chat.
              </p>
              <div className="flex justify-center gap-4 flex-col sm:flex-row">
                <CTAButton to="/signup" primary>
                  <MessageSquareMore className="h-5 w-5" />
                  Start Chatting Securely
                </CTAButton>
                <CTAButton to="/login">Log In</CTAButton>
              </div>
            </div>
          </div>
        </div>
      </section>

      <Footer />
    </div>
  );
}
