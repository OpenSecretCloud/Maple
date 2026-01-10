import { createFileRoute } from "@tanstack/react-router";
import { TopNav } from "@/components/TopNav";
import { FullPageMain } from "@/components/FullPageMain";
import { MarketingHeader } from "@/components/MarketingHeader";
import { Monitor, Terminal, Globe, Smartphone } from "lucide-react";
import { Apple } from "@/components/icons/Apple";
import { Android } from "@/components/icons/Android";
import { useState, useEffect } from "react";
import { getLatestDownloadInfo } from "@/utils/githubRelease";
import { isIOS } from "@/utils/platform";
import packageJson from "../../package.json";

interface DownloadUrls {
  macOS: string;
  linuxAppImage: string;
  linuxDeb: string;
  linuxRpm: string;
  androidApk: string;
}

// Fallback to package.json version if GitHub API fails
const FALLBACK_VERSION = packageJson.version;
const FALLBACK_TAG = `v${FALLBACK_VERSION}`;
const FALLBACK_BASE_URL = `https://github.com/OpenSecretCloud/Maple/releases/download/${FALLBACK_TAG}`;
const FALLBACK_URLS: DownloadUrls = {
  macOS: `${FALLBACK_BASE_URL}/Maple_${FALLBACK_VERSION}_universal.dmg`,
  linuxAppImage: `${FALLBACK_BASE_URL}/Maple_${FALLBACK_VERSION}_amd64.AppImage`,
  linuxDeb: `${FALLBACK_BASE_URL}/Maple_${FALLBACK_VERSION}_amd64.deb`,
  linuxRpm: `${FALLBACK_BASE_URL}/Maple_${FALLBACK_VERSION}_x86_64.rpm`,
  androidApk: `${FALLBACK_BASE_URL}/app-universal-release.apk`
};

function DownloadPage() {
  const isIOSPlatform = isIOS();
  const [downloadUrls, setDownloadUrls] = useState<DownloadUrls>(FALLBACK_URLS);
  const [currentVersion, setCurrentVersion] = useState<string>(FALLBACK_VERSION);
  const [releaseUrl, setReleaseUrl] = useState<string>(
    `https://github.com/OpenSecretCloud/Maple/releases/tag/${FALLBACK_TAG}`
  );
  const [isLoading, setIsLoading] = useState<boolean>(true);

  useEffect(() => {
    async function loadLatestRelease() {
      try {
        const downloadInfo = await getLatestDownloadInfo();
        if (downloadInfo) {
          setDownloadUrls(downloadInfo.downloadUrls);
          setCurrentVersion(downloadInfo.version);
          setReleaseUrl(downloadInfo.releaseUrl);
        }
      } catch (error) {
        console.error("Failed to load latest release info:", error);
        // Keep fallback values
      } finally {
        setIsLoading(false);
      }
    }

    loadLatestRelease();
  }, []);

  return (
    <>
      <TopNav />
      <FullPageMain>
        <MarketingHeader
          title={
            <span className="text-6xl font-light mb-0">
              <span className="text-[hsl(var(--purple))]">Download</span> Maple
            </span>
          }
          subtitle={
            <div className="space-y-2">
              <p>Access your private AI chat with end-to-end encryption</p>
            </div>
          }
        />

        {/* Desktop Downloads Section */}
        <section className="w-full max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-12">
          <h2 className="text-3xl font-light mb-8 flex items-center gap-3">
            <Monitor className="w-7 h-7" /> Desktop Apps
            <span className="ml-3 text-xl text-[hsl(var(--marketing-text-muted))]">
              A faster, more focused experience for your private AI chat
            </span>
          </h2>

          <div className="grid grid-cols-1 md:grid-cols-2 gap-6 max-w-3xl mx-auto">
            <div className="flex flex-col border border-[hsl(var(--marketing-card-border))] bg-[hsl(var(--marketing-card))]/75 text-foreground p-6 rounded-lg hover:border-foreground/30 transition-all duration-300">
              <div className="p-3 rounded-full bg-[hsl(var(--marketing-card))]/50 border border-[hsl(var(--purple))]/30 w-fit mb-4">
                <Apple className="w-6 h-6 text-[hsl(var(--purple))]" />
              </div>
              <h3 className="text-xl font-medium mb-2">macOS</h3>
              <p className="text-[hsl(var(--marketing-text-muted))] mb-6 flex-grow">
                For Apple Silicon and Intel-based Macs running macOS 11.0 or later.
              </p>
              <div className="flex flex-col gap-3">
                <a
                  href={downloadUrls.macOS}
                  className="py-3 rounded-lg text-center font-medium transition-all duration-300 
                  dark:bg-white/90 dark:text-black dark:hover:bg-[hsl(var(--purple))]/80 dark:hover:text-[hsl(var(--foreground))] dark:active:bg-white/80
                  bg-background text-foreground hover:bg-[hsl(var(--purple))] hover:text-[hsl(var(--foreground))] active:bg-background/80 
                  border border-[hsl(var(--purple))]/30 hover:border-[hsl(var(--purple))]
                  shadow-[0_0_15px_rgba(var(--purple-rgb),0.2)] hover:shadow-[0_0_25px_rgba(var(--purple-rgb),0.3)]"
                >
                  Download for macOS
                </a>
                <div className="text-center text-xs text-[hsl(var(--marketing-text-muted))]">
                  Universal for both Apple Silicon and Intel
                </div>
              </div>
            </div>

            <div className="flex flex-col border border-[hsl(var(--marketing-card-border))] bg-[hsl(var(--marketing-card))]/75 text-foreground p-6 rounded-lg hover:border-foreground/30 transition-all duration-300">
              <div className="p-3 rounded-full bg-[hsl(var(--marketing-card))]/50 border border-[hsl(var(--purple))]/30 w-fit mb-4">
                <Terminal className="w-6 h-6 text-[hsl(var(--purple))]" />
              </div>
              <h3 className="text-xl font-medium mb-2">Linux</h3>
              <p className="text-[hsl(var(--marketing-text-muted))] mb-6 flex-grow">
                For Ubuntu 24.04+ only. Other Linux distributions are not officially supported.
              </p>
              <div className="flex flex-col gap-3">
                <a
                  href={downloadUrls.linuxAppImage}
                  className="py-3 rounded-lg text-center font-medium transition-all duration-300 
                  dark:bg-white/90 dark:text-black dark:hover:bg-[hsl(var(--purple))]/80 dark:hover:text-[hsl(var(--foreground))] dark:active:bg-white/80
                  bg-background text-foreground hover:bg-[hsl(var(--purple))] hover:text-[hsl(var(--foreground))] active:bg-background/80 
                  border border-[hsl(var(--purple))]/30 hover:border-[hsl(var(--purple))]
                  shadow-[0_0_15px_rgba(var(--purple-rgb),0.2)] hover:shadow-[0_0_25px_rgba(var(--purple-rgb),0.3)]"
                >
                  Download AppImage
                </a>
                <div className="grid grid-cols-2 gap-2">
                  <a
                    href={downloadUrls.linuxDeb}
                    className="py-2 rounded-lg text-center text-sm font-medium transition-all duration-300 
                    dark:bg-white/90 dark:text-black dark:hover:bg-[hsl(var(--purple))]/80 dark:hover:text-[hsl(var(--foreground))] dark:active:bg-white/80
                    bg-background text-foreground hover:bg-[hsl(var(--purple))] hover:text-[hsl(var(--foreground))] active:bg-background/80 
                    border border-[hsl(var(--purple))]/30 hover:border-[hsl(var(--purple))]"
                  >
                    .deb
                  </a>
                  <a
                    href={downloadUrls.linuxRpm}
                    className="py-2 rounded-lg text-center text-sm font-medium transition-all duration-300 
                    dark:bg-white/90 dark:text-black dark:hover:bg-[hsl(var(--purple))]/80 dark:hover:text-[hsl(var(--foreground))] dark:active:bg-white/80
                    bg-background text-foreground hover:bg-[hsl(var(--purple))] hover:text-[hsl(var(--foreground))] active:bg-background/80 
                    border border-[hsl(var(--purple))]/30 hover:border-[hsl(var(--purple))]"
                  >
                    .rpm
                  </a>
                </div>
              </div>
            </div>
          </div>

          <div className="text-center mt-8 space-y-2 text-[hsl(var(--marketing-text-muted))] max-w-2xl mx-auto">
            <p>
              Windows version coming soon. In the meantime, you can use the{" "}
              <a href="/login" className="text-[hsl(var(--purple))] hover:underline">
                web app
              </a>{" "}
              for full functionality.
            </p>
            <p className="text-sm">
              Current version: <span className="font-mono text-foreground">{currentVersion}</span>
              {isLoading && (
                <span className="text-[hsl(var(--marketing-text-muted))]"> (loading...)</span>
              )}{" "}
              •{" "}
              <a
                href={releaseUrl}
                className="text-[hsl(var(--purple))] hover:underline"
                target="_blank"
                rel="noopener noreferrer"
              >
                Release notes
              </a>
            </p>
          </div>
        </section>

        {/* Mobile Apps Section */}
        <section className="w-full max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-6 mb-12">
          <div className="flex items-center gap-3 mb-6">
            <Smartphone className="w-7 h-7" />
            <span className="text-3xl font-light">Mobile Apps</span>
          </div>
          <div className="grid grid-cols-1 md:grid-cols-2 gap-6 max-w-3xl mx-auto">
            <div className="flex flex-col border border-[hsl(var(--marketing-card-border))] bg-[hsl(var(--marketing-card))]/75 text-foreground p-6 rounded-lg hover:border-foreground/30 transition-all duration-300">
              <div className="p-3 rounded-full bg-[hsl(var(--marketing-card))]/50 border border-[hsl(var(--purple))]/30 w-fit mb-4">
                <Apple className="w-6 h-6 text-[hsl(var(--purple))]" />
              </div>
              <h3 className="text-xl font-medium mb-2">iOS</h3>
              <p className="text-[hsl(var(--marketing-text-muted))] mb-6 flex-grow">
                Download our native iOS app for iPhones and iPads.
              </p>
              <div className="flex flex-col items-center">
                <div className="h-[80px] flex items-center justify-center">
                  <a
                    href="https://apps.apple.com/us/app/id6743764835"
                    className="inline-block"
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    <img
                      src="/app-store-badge.svg"
                      alt="Download on the App Store"
                      className="h-12"
                    />
                  </a>
                </div>
                <div className="w-full border-t border-[hsl(var(--marketing-card-border))] pt-4 mt-4">
                  <p className="text-[hsl(var(--marketing-text-muted))] text-sm mb-3 text-center">
                    Want to test the latest features before they hit the App Store? Join our beta
                    program.
                  </p>
                  <div className="flex justify-center">
                    <a
                      href="https://testflight.apple.com/join/zjgtyAeD"
                      className="inline-block"
                      target="_blank"
                      rel="noopener noreferrer"
                    >
                      <img
                        src="/testflight-badge.png"
                        alt="Available on TestFlight"
                        className="h-12"
                      />
                    </a>
                  </div>
                </div>
              </div>
            </div>
            {!isIOSPlatform && (
              <div className="flex flex-col border border-[hsl(var(--marketing-card-border))] bg-[hsl(var(--marketing-card))]/75 text-foreground p-6 rounded-lg hover:border-foreground/30 transition-all duration-300">
                <div className="p-3 rounded-full bg-[hsl(var(--marketing-card))]/50 border border-[hsl(var(--purple))]/30 w-fit mb-4">
                  <Android className="w-6 h-6 text-[hsl(var(--purple))]" />
                </div>
                <h3 className="text-xl font-medium mb-2">Android</h3>
                <p className="text-[hsl(var(--marketing-text-muted))] mb-6 flex-grow">
                  Download our native Android app for phones and tablets.
                </p>
                <div className="flex flex-col items-center">
                  <div className="h-[80px] flex flex-col items-center justify-center gap-1">
                    <a
                      href="https://play.google.com/store/apps/details?id=cloud.opensecret.maple"
                      className="inline-block"
                      target="_blank"
                      rel="noopener noreferrer"
                    >
                      <img
                        src="/google-play-badge.png"
                        alt="Get it on Google Play"
                        className="h-12"
                      />
                    </a>
                    <a
                      href={downloadUrls.androidApk}
                      className="text-xs text-[hsl(var(--marketing-text-muted))] hover:text-foreground hover:underline"
                      target="_blank"
                      rel="noopener noreferrer"
                    >
                      or download APK directly
                    </a>
                  </div>
                  <div className="w-full border-t border-[hsl(var(--marketing-card-border))] pt-4 mt-4">
                    <p className="text-[hsl(var(--marketing-text-muted))] text-sm mb-5 text-center">
                      Want to test the latest features before they hit the Play Store? Join our beta
                      program.
                    </p>
                    <div className="flex flex-col items-center gap-2">
                      <a
                        href="https://play.google.com/apps/testing/cloud.opensecret.maple"
                        className="py-2 px-4 rounded-lg text-center text-sm font-medium transition-all duration-300
                        dark:bg-white/90 dark:text-black dark:hover:bg-[hsl(var(--purple))]/80 dark:hover:text-[hsl(var(--foreground))] dark:active:bg-white/80
                        bg-background text-foreground hover:bg-[hsl(var(--purple))] hover:text-[hsl(var(--foreground))] active:bg-background/80
                        border border-[hsl(var(--purple))]/30 hover:border-[hsl(var(--purple))]"
                        target="_blank"
                        rel="noopener noreferrer"
                      >
                        Join Google Play Beta
                      </a>
                    </div>
                  </div>
                </div>
              </div>
            )}
          </div>
        </section>

        {/* Web Access Section */}
        <section className="w-full max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-12 mb-12">
          <div className="flex flex-col p-4 sm:p-8 rounded-2xl bg-gradient-to-b from-[hsl(var(--marketing-card))] to-[hsl(var(--marketing-card))]/80 border-2 border-[hsl(var(--purple))] shadow-[0_0_30px_rgba(148,105,248,0.2)]">
            <div className="flex flex-col md:flex-row items-center gap-6 md:gap-8">
              <div className="flex-1">
                <h3 className="text-3xl font-light mb-3">Web Version</h3>
                <p className="text-[hsl(var(--marketing-text-muted))] text-lg mb-6">
                  Don't want to download anything? Use Maple directly in your browser with the same
                  end-to-end encryption.
                </p>
                <a
                  href="/login"
                  className="inline-flex items-center gap-2 py-3 px-6 rounded-lg text-center font-medium transition-all duration-300 
                  dark:bg-white/90 dark:text-black dark:hover:bg-[hsl(var(--purple))]/80 dark:hover:text-[hsl(var(--foreground))] dark:active:bg-white/80
                  bg-background text-foreground hover:bg-[hsl(var(--purple))] hover:text-[hsl(var(--foreground))] active:bg-background/80 
                  border border-[hsl(var(--purple))]/30 hover:border-[hsl(var(--purple))]
                  shadow-[0_0_15px_rgba(var(--purple-rgb),0.2)] hover:shadow-[0_0_25px_rgba(var(--purple-rgb),0.3)]"
                >
                  <Globe className="w-5 h-5" />
                  Open Web App
                </a>
              </div>
              <div className="flex-1 flex justify-center w-full mt-8 md:mt-0">
                <div className="w-full max-w-[380px] aspect-video bg-[hsl(var(--background))] rounded-lg border border-[hsl(var(--border))] overflow-hidden shadow-xl mx-auto">
                  <div className="h-8 border-b border-[hsl(var(--border))] bg-[hsl(var(--background))] flex items-center px-4">
                    <div className="flex gap-1.5">
                      <div className="w-3 h-3 rounded-full bg-red-500/50"></div>
                      <div className="w-3 h-3 rounded-full bg-yellow-500/50"></div>
                      <div className="w-3 h-3 rounded-full bg-green-500/50"></div>
                    </div>
                    <div className="mx-auto text-xs text-[hsl(var(--muted-foreground))]">
                      trymaple.ai
                    </div>
                  </div>
                  <div className="p-2 h-[calc(100%-32px)] flex">
                    <div className="w-1/4 border-r border-[hsl(var(--border))]"></div>
                    <div className="w-3/4 p-2">
                      <div className="h-3 bg-[hsl(var(--muted))]/40 rounded-full w-3/4 mb-2"></div>
                      <div className="h-3 bg-[hsl(var(--muted))]/40 rounded-full w-1/2 mb-4"></div>
                      <div className="bg-[hsl(var(--purple))]/10 rounded p-2 mb-2">
                        <div className="h-2 bg-[hsl(var(--muted))]/40 rounded-full w-full mb-1"></div>
                        <div className="h-2 bg-[hsl(var(--muted))]/40 rounded-full w-3/4"></div>
                      </div>
                      <div className="bg-[hsl(var(--muted))]/10 rounded p-2">
                        <div className="h-2 bg-[hsl(var(--muted))]/40 rounded-full w-full mb-1"></div>
                        <div className="h-2 bg-[hsl(var(--muted))]/40 rounded-full w-1/2"></div>
                      </div>
                    </div>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </section>
      </FullPageMain>
    </>
  );
}

export const Route = createFileRoute("/downloads")({
  component: DownloadPage
});
