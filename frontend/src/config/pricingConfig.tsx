import { Check, X } from "lucide-react";

export type PlanFeature = {
  text: string;
  included: boolean;
  icon?: React.ReactNode;
};

export type PricingPlan = {
  name: string;
  price: string;
  description: string;
  features: PlanFeature[];
  ctaText: string;
  popular?: boolean;
};

export const PRICING_PLANS: PricingPlan[] = [
  {
    name: "Free",
    price: "$0",
    description: "Try Maple with limited usage",
    features: [
      {
        text: "25 messages per week",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      {
        text: "End-to-end encryption",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      {
        text: "Access to core AI features",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      {
        text: "Search Chat History",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      { text: "Rename Chats", included: true, icon: <Check className="w-4 h-4 text-green-500" /> },
      { text: "Image Upload", included: false, icon: <X className="w-4 h-4 text-red-500" /> },
      { text: "Document Upload", included: false, icon: <X className="w-4 h-4 text-red-500" /> },
      { text: "DeepSeek R1 70B", included: false, icon: <X className="w-4 h-4 text-red-500" /> },
      { text: "Gemma 3 27B", included: false, icon: <X className="w-4 h-4 text-red-500" /> },
      {
        text: "Mistral Small 3.1 24B",
        included: false,
        icon: <X className="w-4 h-4 text-red-500" />
      },
      { text: "Qwen 2.5 72B", included: false, icon: <X className="w-4 h-4 text-red-500" /> }
    ],
    ctaText: "Start Free"
  },
  {
    name: "Starter",
    price: "$5.99",
    description: "Get started with secure AI chat",
    features: [
      {
        text: "All features from Free",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      {
        text: "Enough messages for casual use",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      {
        text: "AI Naming of Chats",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      { text: "Gemma 3 27B", included: true, icon: <Check className="w-4 h-4 text-green-500" /> },
      { text: "DeepSeek R1 70B", included: false, icon: <X className="w-4 h-4 text-red-500" /> },
      {
        text: "Mistral Small 3.1 24B",
        included: false,
        icon: <X className="w-4 h-4 text-red-500" />
      },
      { text: "Qwen 2.5 72B", included: false, icon: <X className="w-4 h-4 text-red-500" /> },
      { text: "Image Upload", included: false, icon: <X className="w-4 h-4 text-red-500" /> },
      { text: "Document Upload", included: false, icon: <X className="w-4 h-4 text-red-500" /> }
    ],
    ctaText: "Start Chatting"
  },
  {
    name: "Pro",
    price: "$20",
    description: "For power users who need more",
    features: [
      {
        text: "All features from Free",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      {
        text: "Generous usage for power users",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      { text: "Image Upload", included: true, icon: <Check className="w-4 h-4 text-green-500" /> },
      {
        text: "Document Upload",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      {
        text: "DeepSeek R1 70B",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      { text: "Gemma 3 27B", included: true, icon: <Check className="w-4 h-4 text-green-500" /> },
      {
        text: "Mistral Small 3.1 24B",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      {
        text: "Qwen 2.5 72B",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      }
    ],
    ctaText: "Start Chatting",
    popular: true
  },
  {
    name: "Max",
    price: "$100",
    description: "Maximum usage for power users",
    features: [
      {
        text: "All features from Pro",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      {
        text: "20x more usage than Pro",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      {
        text: "Priority support",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      {
        text: "Early Access to features and models",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      { text: "Image Upload", included: true, icon: <Check className="w-4 h-4 text-green-500" /> },
      {
        text: "Document Upload",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      {
        text: "DeepSeek R1 70B",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      { text: "Gemma 3 27B", included: true, icon: <Check className="w-4 h-4 text-green-500" /> },
      {
        text: "Mistral Small 3.1 24B",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      {
        text: "Qwen 2.5 72B",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      }
    ],
    ctaText: "Start Chatting"
  },
  {
    name: "Team",
    price: "$30",
    description: "For teams and businesses",
    features: [
      {
        text: "All features from Pro",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      {
        text: "2x more usage than Pro per team member",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      {
        text: "Priority support",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      {
        text: "Unified billing",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      {
        text: "Pool chat credits among team",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      {
        text: "Early Access to features and models",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      { text: "Image Upload", included: true, icon: <Check className="w-4 h-4 text-green-500" /> },
      {
        text: "Document Upload",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      {
        text: "DeepSeek R1 70B",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      { text: "Gemma 3 27B", included: true, icon: <Check className="w-4 h-4 text-green-500" /> },
      {
        text: "Mistral Small 3.1 24B",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      },
      {
        text: "Qwen 2.5 72B",
        included: true,
        icon: <Check className="w-4 h-4 text-green-500" />
      }
    ],
    ctaText: "Contact Us"
  }
];
