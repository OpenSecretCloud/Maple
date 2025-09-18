import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Image, Mic, Sparkles, Check, Cpu, Volume2, FileText } from "lucide-react";
import { useNavigate } from "@tanstack/react-router";

interface UpgradePromptDialogProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  feature: "image" | "voice" | "model" | "tts" | "document";
  modelName?: string;
}

export function UpgradePromptDialog({
  open,
  onOpenChange,
  feature,
  modelName
}: UpgradePromptDialogProps) {
  const navigate = useNavigate();

  const handleUpgrade = () => {
    onOpenChange(false);
    navigate({ to: "/pricing" });
  };

  const getFeatureInfo = () => {
    if (feature === "image") {
      return {
        icon: <Image className="h-8 w-8" />,
        title: "Image Upload",
        description: "Upload and analyze images with AI-powered vision models",
        requiredPlan: "Starter",
        benefits: [
          "Images stay private with end-to-end encryption",
          "Upload JPEG, PNG, and WebP formats securely",
          "Use advanced vision models like Gemma 3",
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
    } else if (feature === "tts") {
      return {
        icon: <Volume2 className="h-8 w-8" />,
        title: "Text-to-Speech",
        description: "Listen to AI responses with natural-sounding voices",
        requiredPlan: "Pro",
        benefits: [
          "Audio generation happens privately on secure servers",
          "Natural-sounding AI voices",
          "Perfect for accessibility or multitasking",
          "Listen to long responses hands-free"
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
    } else {
      return {
        icon: <Cpu className="h-8 w-8" />,
        title: modelName ? `Access ${modelName}` : "Powerful AI Models",
        description: "Get access to our most advanced AI models for superior performance",
        requiredPlan: "Pro",
        benefits: [
          "All models run in secure, encrypted environments",
          "Access to DeepSeek R1 for advanced reasoning",
          "OpenAI GPT-OSS, Mistral, Qwen, and more",
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
              Available with Pro plan and above
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

          <div className="pt-2 border-t">
            <p className="text-sm text-muted-foreground">
              Plus access to 6 powerful models (including DeepSeek R1), API access, and more usage
            </p>
          </div>
        </div>

        <DialogFooter className="gap-2 sm:gap-0">
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            Maybe Later
          </Button>
          <Button onClick={handleUpgrade} className="gap-2">
            <Sparkles className="h-4 w-4" />
            View Plans
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
