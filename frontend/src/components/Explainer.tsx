import { ArrowRight, BotIcon, LockIcon, MinusIcon, ServerIcon, SmartphoneIcon } from "lucide-react";

function ArrowAndLock() {
  return (
    <>
      <div className="flex pt-2 -mx-2 max-sm:hidden">
        <MinusIcon className="h-4 w-4 text-muted-foreground" />
        <LockIcon className="h-4 w-4 text-muted-foreground" />
        <ArrowRight className="h-4 w-4 text-muted-foreground" />
      </div>
      {/* only visible on mobile */}
      <div className="flex flex-col py-2 sm:hidden ">
        <MinusIcon className="h-4 w-4 text-muted-foreground rotate-90" />
        <LockIcon className="h-4 w-4 text-muted-foreground" />
        <ArrowRight className="h-4 w-4 text-muted-foreground rotate-90" />
      </div>
    </>
  );
}

export function InfoContent() {
  return (
    <>
      <p className="text-center">
        Encrypted. At every step.
        <br />
        Nobody can read your chats but you.
      </p>
      <div className="flex flex-col items-center justify-center pt-4 sm:flex-row sm:items-start">
        <div className="flex flex-col gap-2 items-center w-[6rem]">
          <SmartphoneIcon className="h-8 w-8 text-primary" />
          <span className="text-center text-sm text-muted-foreground">Your device</span>
        </div>
        <ArrowAndLock />
        <div className="flex flex-col gap-2 items-center w-[6rem]">
          <ServerIcon className="h-8 w-8 text-primary" />
          <span className="text-center text-sm text-muted-foreground">Secure server</span>
        </div>
        <ArrowAndLock />
        <div className="flex flex-col gap-2 items-center w-[6rem]">
          <BotIcon className="h-8 w-8 text-primary" />
          <span className="text-center text-sm text-muted-foreground">AI cloud</span>
        </div>
      </div>
      <div className="w-full pt-4 flex justify-center">
        <a
          href="https://blog.trymaple.ai"
          className="text-center hover:underline font-medium text-sm"
          target="_blank"
          rel="noopener noreferrer"
        >
          Learn more
        </a>
      </div>
    </>
  );
}
