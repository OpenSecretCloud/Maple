export function Footer() {
  return (
    <div className="text-center">
      <h3 className="text-white text-2xl pt-4 font-light">
        Powered by{" "}
        <a
          href="https://opensecret.cloud"
          target="_blank"
          rel="noopener noreferrer"
          className="underline"
        >
          OpenSecret
        </a>
      </h3>
      <div className="mt-2 text-sm text-white/70">
        <a
          href="https://opensecret.cloud/terms"
          target="_blank"
          rel="noopener noreferrer"
          className="hover:text-white/90"
        >
          Terms of Service
        </a>
        {" | "}
        <a
          href="https://opensecret.cloud/privacy"
          target="_blank"
          rel="noopener noreferrer"
          className="hover:text-white/90"
        >
          Privacy Policy
        </a>
      </div>
    </div>
  );
}
