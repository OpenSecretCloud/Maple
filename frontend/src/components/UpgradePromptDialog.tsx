import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import {
  Image,
  Mic,
  Sparkles,
  Check,
  Cpu,
  FileText,
  Gauge,
  MessageCircle,
  Globe
} from "lucide-react";
import { useNavigate } from "@tanstack/react-router";
import { useLocalState } from "@/state/useLocalState";

interface UpgradePromptDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  feature: "image" | "voice" | "model" | "document" | "usage" | "tokens" | "websearch";
  modelName?: string;
}

export function UpgradePromptDialog({
  open,
  onOpenChange,
  feature,
  modelName
}: UpgradePromptDialogProps) {
  const navigate = useNavigate();
  const localState = useLocalState();

  const handleUpgrade = () => {
    onOpenChange(false);
    navigate({ to: "/pricing" });
  };

  const handleNewChat = () => {
    onOpenChange(false);
    // Trigger new chat event
    window.dispatchEvent(new Event("newchat"));
    // Clear the URL
    const params = new URLSearchParams(window.location.search);
    params.delete("conversation_id");
    window.history.replaceState({}, "", params.toString() ? `/?${params}` : "/");
  };

  // Determine user's current plan and next upgrade tier
  const currentPlan = localState.billingStatus?.product_name?.toLowerCase() || "free";
  const isFreeTier = !localState.billingStatus?.product_name || currentPlan === "free";
  const isPro = currentPlan.includes("pro") && !currentPlan.includes("max");
  const isMax = currentPlan.includes("max");

  const getNextPlan = () => {
    if (isFreeTier) return "Pro";
    if (isPro) return "Max";
    return "Max"; // Already on Max or Team
  };

  const getFeatureInfo = () => {
    if (feature === "websearch") {
      return {
        icon: <Globe className="h-8 w-8" />,
        title: "Live Web Search",
        description: "Search the web in real-time with AI-powered results",
        requiredPlan: "Pro",
        benefits: [
          "Live web search powered by Brave",
          "Get up-to-date information from the internet",
          "Search queries are sent to Brave but not linked to your identity",
          "Results are processed privately and securely",
          "Perfect for current events, research, and fact-checking",
          "Seamlessly integrated into your chat experience"
        ]
      };
    } else if (feature === "image") {
      return {
        icon: <Image className="h-8 w-8" />,
        title: "Image Upload",
        description: "Upload and analyze images with AI-powered vision models",
        requiredPlan: "Starter",
        benefits: [
          "Images stay private with end-to-end encryption",
          "Upload JPEG, PNG, and WebP formats securely",
          "Use advanced vision models like Gemma 3 and Qwen3-VL",
          "Analyze diagrams, screenshots, and photos privately",
          "Extract text from images without exposing data"
        ]
      };
    } else if (feature === "voice") {
      return {
        icon: <Mic className="h-8 w-8" />,
        title: "Voice Recording",
        description: "Record and transcribe voice messages with Whisper AI",
        requiredPlan: "Pro",
        benefits: [
          "Voice recordings are end-to-end encrypted",
          "Record messages directly in chat securely",
          "Private transcription with Whisper Large v3",
          "Support for multiple languages",
          "No audio data is stored or used for training"
        ]
      };
    } else if (feature === "document") {
      return {
        icon: <FileText className="h-8 w-8" />,
        title: "Document Upload",
        description: "Process and analyze documents with complete privacy",
        requiredPlan: "Pro",
        benefits: [
          "Documents are processed securely with end-to-end encryption",
          "Your files are never stored or used for AI training",
          "Support for PDF, TXT, and Markdown formats",
          "Extract and analyze text while maintaining complete privacy",
          "Local processing ensures your sensitive data never leaves your device"
        ]
      };
    } else if (feature === "usage") {
      const nextPlan = getNextPlan();
      return {
        icon: <Gauge className="h-8 w-8" />,
        title: isFreeTier ? "Daily Usage Limit Reached" : "Monthly Usage Limit Reached",
        description: isFreeTier
          ? "You've reached your daily free tier limit. Upgrade to Pro for unlimited daily usage."
          : isPro
            ? "You've reached your Pro plan's monthly limit. Upgrade to Max for 10x more usage."
            : "You've reached your monthly usage limit. Please wait for the next billing cycle.",
        requiredPlan: nextPlan,
        benefits: isFreeTier
          ? [
              "No daily limits with Pro plan",
              "Access to advanced AI models",
              "Priority access during peak times",
              "Higher monthly rate limits for continuous usage",
              "Process images and documents",
              "API access for developers"
            ]
          : isPro
            ? [
                "10x more monthly messages with Max plan",
                "Access to all premium models including DeepSeek R1",
                "Highest priority during peak times",
                "Maximum rate limits for power users"
              ]
            : [
                "You're already on our highest individual plan",
                "Consider Team plans for shared usage",
                "Monthly usage automatically refreshes",
                "Contact support for custom enterprise plans"
              ]
      };
    } else if (feature === "tokens") {
      return {
        icon: <MessageCircle className="h-8 w-8" />,
        title: "Conversation Limit",
        description:
          "This conversation is too long for the free tier. Upgrade to Pro to continue this chat, or start a new conversation.",
        requiredPlan: "Pro",
        benefits: [
          "No conversation limits",
          "Continue conversations without interruption",
          "Process longer documents and code files",
          "Auto-compaction keeps conversations flowing"
        ]
      };
    } else {
      return {
        icon: <Cpu className="h-8 w-8" />,
        title: modelName ? `Access ${modelName}` : "Powerful AI Models",
        description: "Get access to our most advanced AI models for superior performance",
        requiredPlan: "Pro",
        benefits: [
          "All models run in secure, encrypted environments",
          "Access to DeepSeek R1 for advanced reasoning",
          "OpenAI GPT-OSS, Qwen, and other advanced models",
          "Higher token limits for longer conversations",
          "Priority access to new models as they launch"
        ]
      };
    }
  };

  const info = getFeatureInfo();

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-md">
        <DialogHeader>
          <div className="flex items-center gap-3 mb-2">
            <div className="p-2 rounded-lg bg-primary/10 text-primary">{info.icon}</div>
            <DialogTitle>{info.title}</DialogTitle>
          </div>
          <DialogDescription className="text-base">{info.description}</DialogDescription>
        </DialogHeader>

        <div className="space-y-4 py-4">
          <div className="space-y-2">
            <p className="text-sm font-medium text-muted-foreground">
              {info.requiredPlan === "Max" && isMax
                ? "You're on the Max plan"
                : `Available with ${info.requiredPlan} plan${info.requiredPlan !== "Max" ? " and above" : ""}`}
            </p>
            <ul className="space-y-2">
              {info.benefits.map((benefit, i) => (
                <li key={i} className="flex items-start gap-2 text-sm">
                  <Check className="h-4 w-4 text-green-500 mt-0.5 shrink-0" />
                  <span>{benefit}</span>
                </li>
              ))}
            </ul>
          </div>

          {info.requiredPlan !== "Max" || !isMax ? (
            <div className="pt-2 border-t">
              <p className="text-sm text-muted-foreground">
                {isFreeTier
                  ? "Plus access to 7 powerful models, image & document processing, and more"
                  : isPro
                    ? "Plus access to DeepSeek R1, 10x more usage, API access, and priority support"
                    : "Explore our pricing options for the best plan for your needs"}
              </p>
            </div>
          ) : null}
        </div>

        <DialogFooter className="gap-2 sm:gap-0">
          {/* Show "Start New Chat" for free tier conversation limit, "Maybe Later" for others */}
          {feature === "tokens" && isFreeTier ? (
            <Button variant="outline" onClick={handleNewChat}>
              Start New Chat
            </Button>
          ) : (
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              Maybe Later
            </Button>
          )}
          {(info.requiredPlan !== "Max" || !isMax) && (
            <Button onClick={handleUpgrade} className="gap-2">
              <Sparkles className="h-4 w-4" />
              {isFreeTier ? "Upgrade to Pro" : isPro ? "Upgrade to Max" : "View Plans"}
            </Button>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
