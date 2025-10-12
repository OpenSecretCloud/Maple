import { Check, X, HelpCircle, DollarSign, MinusCircle } from "lucide-react";

interface ComparisonData {
  feature: string;
  maple: string;
  lumo: string;
  duckAI: string;
  chatGPT: string;
  claude: string;
  grok: string;
}

const comparisonData: ComparisonData[] = [
  {
    feature: "Open-Source Code Full Stack",
    maple: "Yes",
    lumo: "No",
    duckAI: "No",
    chatGPT: "No",
    claude: "No",
    grok: "No"
  },
  {
    feature: "Mathematic Proof That Cloud Code matches Source",
    maple: "Yes",
    lumo: "No",
    duckAI: "No",
    chatGPT: "No",
    claude: "No",
    grok: "No"
  },
  {
    feature: "Can't use your data to train AI",
    maple: "Yes",
    lumo: "?",
    duckAI: "?",
    chatGPT: "No",
    claude: "No",
    grok: "No"
  },
  {
    feature: "Doesn't log your chats",
    maple: "Yes",
    lumo: "?",
    duckAI: "?",
    chatGPT: "No",
    claude: "No",
    grok: "No"
  },
  {
    feature: "Can't share your data",
    maple: "Yes",
    lumo: "?",
    duckAI: "?",
    chatGPT: "No",
    claude: "No",
    grok: "No"
  },
  {
    feature: "Zero Data Retention",
    maple: "Yes",
    lumo: "?",
    duckAI: "?",
    chatGPT: "$",
    claude: "$",
    grok: "No"
  },
  {
    feature: "Mobile apps",
    maple: "Yes",
    lumo: "Yes",
    duckAI: "No",
    chatGPT: "Yes",
    claude: "Yes",
    grok: "Yes"
  },
  {
    feature: "Open Models",
    maple: "Yes",
    lumo: "Yes",
    duckAI: "Partial",
    chatGPT: "No",
    claude: "No",
    grok: "No"
  },
  {
    feature: "Coding Models",
    maple: "Yes",
    lumo: "No",
    duckAI: "Yes",
    chatGPT: "Yes",
    claude: "Yes",
    grok: "Yes"
  },
  {
    feature: "Integration With Coding IDE",
    maple: "Yes",
    lumo: "No",
    duckAI: "No",
    chatGPT: "Yes",
    claude: "Yes",
    grok: "No"
  },
  {
    feature: "Teams Plans",
    maple: "Yes",
    lumo: "No",
    duckAI: "No",
    chatGPT: "Yes",
    claude: "Yes",
    grok: "No"
  },
  {
    feature: "Developer API",
    maple: "Yes",
    lumo: "No",
    duckAI: "No",
    chatGPT: "Yes",
    claude: "Yes",
    grok: "No"
  },
  {
    feature: "Sends your data to non-private AI providers",
    maple: "No",
    lumo: "Yes",
    duckAI: "Yes",
    chatGPT: "Yes",
    claude: "Yes",
    grok: "Yes"
  }
];

const ValueIcon = ({ value }: { value: string }) => {
  switch (value) {
    case "Yes":
      return (
        <div className="flex items-center justify-center w-6 h-6 bg-green-100 dark:bg-green-900/30 rounded-full">
          <Check className="w-4 h-4 text-green-600 dark:text-green-400" aria-label="Available" />
        </div>
      );
    case "No":
      return (
        <div className="flex items-center justify-center w-6 h-6 bg-red-100 dark:bg-red-900/30 rounded-full">
          <X className="w-4 h-4 text-red-600 dark:text-red-400" aria-label="Not Available" />
        </div>
      );
    case "?":
      return (
        <div className="flex items-center justify-center w-6 h-6 bg-yellow-100 dark:bg-yellow-900/30 rounded-full">
          <HelpCircle
            className="w-4 h-4 text-yellow-600 dark:text-yellow-400"
            aria-label="Unknown or Unclear"
          />
        </div>
      );
    case "$":
      return (
        <div className="flex items-center justify-center w-6 h-6 bg-blue-100 dark:bg-blue-900/30 rounded-full">
          <DollarSign
            className="w-4 h-4 text-blue-600 dark:text-blue-400"
            aria-label="Paid Feature"
          />
        </div>
      );
    case "Partial":
      return (
        <div className="flex items-center justify-center w-6 h-6 bg-orange-100 dark:bg-orange-900/30 rounded-full">
          <MinusCircle
            className="w-4 h-4 text-orange-600 dark:text-orange-400"
            aria-label="Partially Available"
          />
        </div>
      );
    default:
      return <span className="text-sm text-foreground/70">{value}</span>;
  }
};

export function ComparisonChart() {
  const products = [
    { key: "maple", name: "Maple", highlight: true },
    { key: "lumo", name: "Lumo", highlight: false },
    { key: "duckAI", name: "Duck AI", highlight: false },
    { key: "chatGPT", name: "ChatGPT", highlight: false },
    { key: "claude", name: "Claude", highlight: false },
    { key: "grok", name: "Grok", highlight: false }
  ];

  return (
    <section className="w-full py-20">
      <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
        <div className="text-center mb-16">
          <h2 className="text-4xl font-light mb-4">
            How We <span className="text-[hsl(var(--purple))] font-medium">Compare</span>
          </h2>
          <p className="text-xl text-[hsl(var(--marketing-text-muted))] max-w-2xl mx-auto">
            See how Maple stacks up against other AI chat platforms when it comes to privacy,
            security, and developer features.
          </p>
        </div>

        <div className="overflow-x-auto">
          <div className="inline-block min-w-full">
            <div className="bg-[hsl(var(--marketing-card))]/80 backdrop-blur-sm border border-[hsl(var(--marketing-card-border))] rounded-xl overflow-hidden">
              {/* Header Row */}
              <div
                className="grid border-b border-[hsl(var(--marketing-card-border))]"
                style={{ gridTemplateColumns: "2fr repeat(6, 1fr)" }}
              >
                <div className="p-3 font-medium text-foreground bg-[hsl(var(--marketing-card-highlight))]/30 text-sm">
                  Feature
                </div>
                {products.map((product) => (
                  <div
                    key={product.key}
                    className={`p-3 text-center font-medium relative ${
                      product.highlight
                        ? "bg-[hsl(var(--purple))]/5 text-[hsl(var(--purple))] border-l-2 border-r-2 border-[hsl(var(--purple))]/30"
                        : "text-foreground"
                    }`}
                  >
                    <span className="text-sm">{product.name}</span>
                  </div>
                ))}
              </div>

              {/* Data Rows */}
              {comparisonData.map((row, index) => (
                <div
                  key={index}
                  className={`grid border-b border-[hsl(var(--marketing-card-border))] ${
                    index % 2 === 0 ? "bg-[hsl(var(--marketing-card-highlight))]/20" : ""
                  }`}
                  style={{ gridTemplateColumns: "2fr repeat(6, 1fr)" }}
                >
                  <div className="p-3 font-medium text-foreground bg-[hsl(var(--marketing-card-highlight))]/30">
                    <span className="text-sm">{row.feature}</span>
                  </div>
                  {products.map((product) => (
                    <div
                      key={product.key}
                      className={`p-3 text-center ${
                        product.highlight
                          ? "bg-[hsl(var(--purple))]/5 border-l-2 border-r-2 border-[hsl(var(--purple))]/30"
                          : ""
                      }`}
                    >
                      <div className="flex justify-center">
                        <ValueIcon value={row[product.key as keyof ComparisonData]} />
                      </div>
                    </div>
                  ))}
                </div>
              ))}
            </div>
          </div>
        </div>

        {/* Legend */}
        <div className="mt-8 flex flex-wrap justify-center gap-6 text-sm text-[hsl(var(--marketing-text-muted))]">
          <div className="flex items-center gap-2">
            <ValueIcon value="Yes" />
            <span>Available</span>
          </div>
          <div className="flex items-center gap-2">
            <ValueIcon value="No" />
            <span>Not Available</span>
          </div>
          <div className="flex items-center gap-2">
            <ValueIcon value="?" />
            <span>Unknown/Unclear</span>
          </div>
          <div className="flex items-center gap-2">
            <ValueIcon value="$" />
            <span>Paid Feature</span>
          </div>
          <div className="flex items-center gap-2">
            <ValueIcon value="Partial" />
            <span>Partially Available</span>
          </div>
        </div>
      </div>
    </section>
  );
}
