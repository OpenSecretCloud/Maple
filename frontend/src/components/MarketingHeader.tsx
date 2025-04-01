type MarketingHeaderProps = {
  title: React.ReactNode;
  subtitle: React.ReactNode;
  className?: string;
};

export function MarketingHeader({ title, subtitle, className = "pt-12" }: MarketingHeaderProps) {
  return (
    <div
      className={`flex flex-col items-center gap-[calc(3rem-0.1em)] text-foreground ${className}`}
    >
      <h2 className="text-[clamp(2.5rem,10vw,5rem)] font-light leading-none tracking-tight text-center text-balance bg-gradient-to-b from-foreground to-foreground/80 inline-block text-transparent bg-clip-text pb-[0.1em]">
        {title}
      </h2>
      <h3 className="text-[clamp(1.5rem,5vw,2.5rem)] text-center leading-tight font-light tracking-tight text-balance">
        {subtitle}
      </h3>
    </div>
  );
}
