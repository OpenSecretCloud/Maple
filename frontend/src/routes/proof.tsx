import { createFileRoute } from "@tanstack/react-router";
import { Loader2 } from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { ParsedAttestationView, useOpenSecret } from "@opensecret/react";
import { TopNav } from "@/components/TopNav";
import { FullPageMain } from "@/components/FullPageMain";
import { MarketingHeader } from "@/components/MarketingHeader";

function MatchIndicator({ isMatch, text = "It's a match!" }: { isMatch: boolean; text?: string }) {
  return (
    <p className={`text-sm my-2 ${isMatch ? "text-green-500" : "text-red-500"}`}>
      {isMatch ? "✓" : "✗"} {text}
    </p>
  );
}

export const Route = createFileRoute("/proof")({
  component: Verify
});

function ProofDisplay({
  parsedDocument,
  os
}: {
  parsedDocument: ParsedAttestationView;
  os: ReturnType<typeof useOpenSecret>;
}) {
  return (
    <div className="flex flex-col gap-4 text-white pt-8">
      <div className="flex flex-col gap-6 border-white/10 bg-black/75 text-white p-8 border rounded-lg">
        <h3 className="text-2xl font-medium">Server PCR0 Fingerprint</h3>

        {parsedDocument.pcrs.map(
          (pcr) =>
            pcr.id === 0 && (
              <div key={pcr.id}>
                <p>
                  <span className="font-mono break-all">{pcr.value}</span>
                </p>

                <MatchIndicator
                  isMatch={parsedDocument.validatedPcr0Hash?.isMatch ?? false}
                  text={parsedDocument.validatedPcr0Hash?.text ?? ""}
                />
              </div>
            )
        )}
        <p className="text-sm text-white/70">
          For technical details, check out the{" "}
          <a
            href="https://docs.aws.amazon.com/enclaves/latest/user/verify-root.html"
            target="_blank"
            rel="noopener noreferrer"
            className="underline hover:text-white/80"
          >
            AWS Nitro Enclaves documentation
          </a>
          .
        </p>
      </div>

      <details className="group rounded p-8 bg-black/75">
        <summary className="font-medium cursor-pointer hover:text-white/80">
          <span className="group-open:hidden">Show</span>
          <span className="hidden group-open:inline">Hide</span>
          {" Full Attestation Details"}
        </summary>
        <div className="mt-4 space-y-4">
          <div className="text-sm text-gray-400 space-y-2">
            <p>
              Module ID: <span className="font-mono break-all">{parsedDocument.moduleId}</span>
            </p>
            <p>Timestamp: {parsedDocument.timestamp}</p>
            {parsedDocument.nonce && (
              <p>
                Nonce: <span className="font-mono break-all">{parsedDocument.nonce}</span>
              </p>
            )}
            <p>Digest: {parsedDocument.digest}</p>
            {parsedDocument.publicKey && (
              <>
                <p>
                  Public Key:{" "}
                  <span className="font-mono break-all">{parsedDocument.publicKey}</span>
                </p>
                <MatchIndicator
                  isMatch={true}
                  text="Document signature verified with this public key"
                />
              </>
            )}
            <div>
              <details className="group mt-2">
                <summary className="text-sm cursor-pointer hover:text-white/80">
                  Additional PCR Values
                </summary>
                <div className="mt-2 space-y-2">
                  {parsedDocument.pcrs.map(
                    (pcr) =>
                      pcr.id !== 0 && (
                        <div key={pcr.id}>
                          <p>
                            PCR{pcr.id}: <span className="font-mono break-all">{pcr.value}</span>
                          </p>
                        </div>
                      )
                  )}
                </div>
              </details>
            </div>
            <div>
              <p className="font-medium mt-4 mb-2">Certificate Chain:</p>
              {parsedDocument.certificates.map((cert, index) => (
                <div key={index} className="mb-4 p-3 bg-black/50 rounded">
                  {cert.isRoot ? (
                    <p className="font-medium mb-1">Root Certificate</p>
                  ) : (
                    <p className="font-medium mb-1">Certificate {index + 1}</p>
                  )}
                  <p className="text-sm">Subject: {cert.subject}</p>
                  <p className="text-sm">Valid From: {cert.notBefore}</p>
                  <p className="text-sm">Valid Until: {cert.notAfter}</p>
                  {cert.isRoot ? (
                    <>
                      <p>
                        Calculated SHA-256:{" "}
                        <span className="font-mono break-all">{parsedDocument.cert0hash}</span>
                      </p>
                      <p>
                        Expected root cert hash:{" "}
                        <span className="font-mono break-all">{os.expectedRootCertHash}</span>
                      </p>
                      <MatchIndicator
                        isMatch={parsedDocument.cert0hash === os.expectedRootCertHash}
                        text={
                          parsedDocument.cert0hash === os.expectedRootCertHash
                            ? "Root certificate hash matches AWS root certificate"
                            : "Root certificate hash does not match!"
                        }
                      />
                    </>
                  ) : (
                    <MatchIndicator isMatch={true} text="Signature verified with chain" />
                  )}
                  <details className="mt-2">
                    <summary className="text-sm cursor-pointer hover:text-white/80">
                      Show PEM Certificate
                    </summary>
                    <pre className="mt-2 p-2 bg-black/50 rounded text-xs font-mono whitespace-pre-wrap break-all">
                      {cert.pem}
                    </pre>
                  </details>
                </div>
              ))}
            </div>
          </div>
        </div>
      </details>
    </div>
  );
}

function Verify() {
  const os = useOpenSecret();
  const {
    data: parsedDocument,
    isLoading,
    error
  } = useQuery<ParsedAttestationView>({
    queryKey: ["raw-attestation"],
    queryFn: async () => {
      return await os.getAttestationDocument();
    },
    retry: false
  });

  console.log("Query state:", { isLoading, error, parsedDocument });

  return (
    <>
      <TopNav />
      <FullPageMain>
        <MarketingHeader
          title="Proof of Security"
          subtitle="Cryptographic proof that you're talking with a secure server."
        />

        {isLoading && (
          <div className="flex items-center justify-center p-8">
            <Loader2 className="h-8 w-8 animate-spin text-muted-foreground" />
          </div>
        )}

        {error && (
          <div className="text-red-700 dark:text-red-500 p-4 rounded bg-red-500/10">
            Failed to validate attestation document:{" "}
            {error instanceof Error ? error.message : "Unknown error"}
          </div>
        )}

        {parsedDocument && <ProofDisplay parsedDocument={parsedDocument} os={os} />}

        {/* </div> */}
      </FullPageMain>
    </>
  );
}
