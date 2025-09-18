import { CornerRightUp, Bot, Image, X, FileText, Loader2, Mic } from "lucide-react";
import RecordRTC from "recordrtc";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { UpgradePromptDialog } from "@/components/UpgradePromptDialog";
import { DocumentPlatformDialog } from "@/components/DocumentPlatformDialog";
import { RecordingOverlay } from "@/components/RecordingOverlay";
import { useEffect, useRef, useState, useMemo } from "react";
import { useLocalState } from "@/state/useLocalState";
import { cn, useIsMobile } from "@/utils/utils";
import { useQuery } from "@tanstack/react-query";
import { getBillingService } from "@/billing/billingService";
import { Route as ChatRoute } from "@/routes/_auth.chat.$chatId";
import { ChatMessage } from "@/state/LocalStateContext";
import { useNavigate, useRouter } from "@tanstack/react-router";
import { ModelSelector, MODEL_CONFIG, getModelTokenLimit } from "@/components/ModelSelector";
import { useOpenSecret } from "@opensecret/react";
import type { DocumentResponse } from "@opensecret/react";
import { encode } from "gpt-tokenizer";

interface ParsedDocument {
  document: {
    filename: string;
    text_content: string | null;
  };
  status: string;
  errors: unknown[];
}

interface RustDocumentResponse {
  document: {
    filename: string;
    text_content: string;
  };
  status: string;
}

// Accurate token counting using gpt-tokenizer
function estimateTokenCount(text: string): number {
  // Use gpt-tokenizer for accurate token counting
  return encode(text).length;
}

// Estimated token count for images (varies by model and image size)
const IMAGE_TOKEN_ESTIMATE = 85;

// Calculate total tokens for messages and current input
function calculateTotalTokens(messages: ChatMessage[], currentInput: string): number {
  return (
    messages.reduce((acc, msg) => {
      if (typeof msg.content === "string") {
        return acc + estimateTokenCount(msg.content);
      } else {
        // For multimodal content, estimate tokens from text parts
        return (
          acc +
          msg.content.reduce((sum, part) => {
            if (part.type === "text") {
              return sum + estimateTokenCount(part.text);
            }
            // Rough estimate for images
            return sum + IMAGE_TOKEN_ESTIMATE;
          }, 0)
        );
      }
    }, 0) + (currentInput ? estimateTokenCount(currentInput) : 0)
  );
}

// Custom hook for debouncing values
function useDebounce<T>(value: T, delay: number): T {
  const [debouncedValue, setDebouncedValue] = useState(value);

  useEffect(() => {
    const handler = setTimeout(() => {
      setDebouncedValue(value);
    }, delay);

    return () => {
      clearTimeout(handler);
    };
  }, [value, delay]);

  return debouncedValue;
}

function TokenWarning({
  chatId,
  className,
  onCompress,
  isCompressing = false,
  tokenPercentage
}: {
  chatId?: string;
  className?: string;
  onCompress?: () => void;
  isCompressing?: boolean;
  tokenPercentage: number;
}) {
  const navigate = useNavigate();

  // Only show warning if above 50%
  if (tokenPercentage < 50) return null;

  // Determine the severity and behavior based on percentage
  const isAt95Percent = tokenPercentage >= 95;
  const isAt99Percent = tokenPercentage >= 99;

  const handleNewChat = async (e: React.MouseEvent) => {
    e.preventDefault();
    try {
      await navigate({ to: "/" });
      // Ensure element is available after navigation
      setTimeout(() => document.getElementById("message")?.focus(), 0);
    } catch (error) {
      console.error("Navigation failed:", error);
    }
  };

  // Get appropriate message and styling based on threshold
  const getMessage = () => {
    if (isAt99Percent) {
      return "This chat is too long to continue.";
    } else if (isAt95Percent) {
      return "Chat is at capacity. Compress to continue.";
    } else {
      return "This chat is getting long. Compress it to save tokens.";
    }
  };

  const getButtonText = () => {
    if (isCompressing) {
      return { desktop: "Compressing...", mobile: "Compressing..." };
    }
    if (onCompress) {
      return { desktop: "Compress chat", mobile: "Compress" };
    }
    return { desktop: "Start a new chat", mobile: "New chat" };
  };

  const buttonText = getButtonText();

  // Determine background color based on severity
  const bgClass = isAt99Percent
    ? "bg-destructive/20 border border-destructive/30"
    : isAt95Percent
      ? "bg-warning/20 border border-warning/30"
      : "bg-muted/50";

  return (
    <div
      className={cn(
        "flex items-center justify-between px-3 py-1.5 mb-1",
        "backdrop-blur-sm rounded-t-lg",
        "text-xs",
        bgClass,
        isAt99Percent
          ? "text-destructive"
          : isAt95Percent
            ? "text-warning-foreground"
            : "text-muted-foreground/90",
        className
      )}
    >
      <div className="flex items-center gap-2 min-w-0">
        <span className="text-[11px] font-semibold shrink-0">
          {isAt99Percent ? "Error:" : isAt95Percent ? "Warning:" : "Tip:"}
        </span>
        <span className="min-w-0">{getMessage()}</span>
      </div>
      {chatId && !isAt99Percent && (
        <button
          onClick={!isCompressing ? onCompress || handleNewChat : undefined}
          disabled={isCompressing}
          className={cn(
            "font-medium transition-colors whitespace-nowrap shrink-0 ml-4",
            isCompressing ? "opacity-70 cursor-default" : "hover:underline",
            isAt99Percent
              ? "text-destructive"
              : isAt95Percent
                ? "text-warning-foreground hover:text-warning-foreground/80"
                : "text-primary hover:text-primary/80"
          )}
        >
          <span className="hidden md:inline">{buttonText.desktop}</span>
          <span className="md:hidden">{buttonText.mobile}</span>
          <span className="sr-only">, to reduce token usage</span>
        </button>
      )}
    </div>
  );
}

export default function Component({
  onSubmit,
  startTall,
  messages = [],
  isStreaming = false,
  onCompress,
  isSummarizing = false,
  imageConversionError
}: {
  onSubmit: (
    input: string,
    systemPrompt?: string,
    images?: File[],
    documentText?: string,
    documentMetadata?: { filename: string; fullContent: string },
    sentViaVoice?: boolean
  ) => void | Promise<void>;
  startTall?: boolean;
  messages?: ChatMessage[];
  isStreaming?: boolean;
  onCompress?: () => void;
  isSummarizing?: boolean;
  imageConversionError?: string | null;
}) {
  const [inputValue, setInputValue] = useState("");
  const [systemPromptValue, setSystemPromptValue] = useState("");
  const [isSystemPromptExpanded, setIsSystemPromptExpanded] = useState(false);
  const {
    billingStatus,
    setBillingStatus,
    draftMessages,
    setDraftMessage,
    clearDraftMessage,
    model,
    setModel,
    availableModels,
    hasWhisperModel
  } = useLocalState();

  const supportsVision = MODEL_CONFIG[model]?.supportsVision || false;
  const [images, setImages] = useState<File[]>([]);
  const [imageUrls, setImageUrls] = useState<Map<File, string>>(new Map());
  const [uploadedDocument, setUploadedDocument] = useState<{
    original: DocumentResponse;
    parsed: ParsedDocument;
    cleanedText: string;
  } | null>(null);
  const [isUploadingDocument, setIsUploadingDocument] = useState(false);
  const [documentError, setDocumentError] = useState<string | null>(null);
  const [imageError, setImageError] = useState<string | null>(null);
  const [isTauriEnv, setIsTauriEnv] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const documentInputRef = useRef<HTMLInputElement>(null);
  const [upgradeDialogOpen, setUpgradeDialogOpen] = useState(false);
  const [upgradeFeature, setUpgradeFeature] = useState<"image" | "voice" | "document">("image");
  const [documentPlatformDialogOpen, setDocumentPlatformDialogOpen] = useState(false);
  const os = useOpenSecret();

  // Audio recording state
  const [isRecording, setIsRecording] = useState(false);
  const [isTranscribing, setIsTranscribing] = useState(false);
  const [isProcessingSend, setIsProcessingSend] = useState(false);
  const [audioError, setAudioError] = useState<string | null>(null);
  const recorderRef = useRef<RecordRTC | null>(null);
  const streamRef = useRef<MediaStream | null>(null);

  // Find the first vision-capable model the user has access to
  const findFirstVisionModel = () => {
    // Check if user has Pro/Team access
    if (!hasProTeamAccess) return null;

    // Find first model that supports vision
    for (const modelId of availableModels.map((m) => m.id)) {
      const modelConfig = MODEL_CONFIG[modelId];
      if (modelConfig?.supportsVision) {
        // Check if user has access to this model
        const needsStarter = modelConfig.requiresStarter;
        const needsPro = modelConfig.requiresPro;

        // If no special requirements, or user meets requirements
        if (!needsStarter && !needsPro) return modelId;
        if (
          needsStarter &&
          (freshBillingStatus?.product_name?.toLowerCase().includes("starter") ||
            freshBillingStatus?.product_name?.toLowerCase().includes("pro") ||
            freshBillingStatus?.product_name?.toLowerCase().includes("max") ||
            freshBillingStatus?.product_name?.toLowerCase().includes("team"))
        ) {
          return modelId;
        }
        if (needsPro && hasProTeamAccess) return modelId;
      }
    }
    return null;
  };

  const handleAddImages = (e: React.ChangeEvent<HTMLInputElement>) => {
    if (!e.target.files) return;

    const supportedTypes = ["image/jpeg", "image/jpg", "image/png", "image/webp"];
    const maxSizeInBytes = 10 * 1024 * 1024; // 10MB for images
    const errors: string[] = [];

    const validFiles = Array.from(e.target.files).filter((file) => {
      // Check file type
      if (!supportedTypes.includes(file.type.toLowerCase())) {
        return false;
      }

      // Check file size
      if (file.size > maxSizeInBytes) {
        const sizeInMB = (file.size / (1024 * 1024)).toFixed(2);
        errors.push(`${file.name} is too large (${sizeInMB}MB)`);
        return false;
      }

      return true;
    });

    if (validFiles.length < e.target.files.length) {
      const skippedCount = e.target.files.length - validFiles.length;
      const typeErrors = e.target.files.length - validFiles.length - errors.length;

      if (errors.length > 0) {
        setImageError(`${errors.join(", ")}. Max size is 10MB per image.`);
      } else if (typeErrors > 0) {
        setImageError(
          `${skippedCount} file(s) skipped. Only JPEG, PNG, and WebP images are supported.`
        );
      }
      // Clear error after 5 seconds
      setTimeout(() => setImageError(null), 5000);
    } else {
      setImageError(null);
    }

    // Create object URLs for the new images
    const newUrlMap = new Map(imageUrls);
    validFiles.forEach((file) => {
      if (!newUrlMap.has(file)) {
        newUrlMap.set(file, URL.createObjectURL(file));
      }
    });
    setImageUrls(newUrlMap);
    setImages((prev) => [...prev, ...validFiles]);
  };

  const removeImage = (idx: number) => {
    setImages((prev) => {
      const fileToRemove = prev[idx];
      // Revoke the object URL when removing the image
      const url = imageUrls.get(fileToRemove);
      if (url) {
        URL.revokeObjectURL(url);
        setImageUrls((prevUrls) => {
          const newUrls = new Map(prevUrls);
          newUrls.delete(fileToRemove);
          return newUrls;
        });
      }
      return prev.filter((_, i) => i !== idx);
    });
    // Clear any image errors when removing images
    setImageError(null);
  };

  // Helper function to read text file and format as ParsedDocument
  const processTextFileLocally = async (file: File): Promise<ParsedDocument> => {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();

      reader.onload = (event) => {
        const content = event.target?.result as string;

        // Create a ParsedDocument structure matching the expected format
        const parsedDocument: ParsedDocument = {
          document: {
            filename: file.name,
            text_content: content
          },
          status: "completed",
          errors: []
        };

        resolve(parsedDocument);
      };

      reader.onerror = () => {
        reject(new Error("Failed to read file"));
      };

      reader.readAsText(file);
    });
  };

  const handleDocumentUpload = async (e: React.ChangeEvent<HTMLInputElement>) => {
    if (!e.target.files || e.target.files.length === 0) return;

    const file = e.target.files[0];

    // Check file size (10MB limit for local processing)
    const maxSizeInBytes = 10 * 1024 * 1024; // 10MB
    if (file.size > maxSizeInBytes) {
      const sizeInMB = (file.size / (1024 * 1024)).toFixed(2);
      setDocumentError(`File too large (${sizeInMB}MB). Maximum size is 10MB.`);
      e.target.value = ""; // Reset input
      return;
    }

    setIsUploadingDocument(true);
    setDocumentError(null);

    try {
      let parsed: ParsedDocument;
      let result: DocumentResponse | undefined;

      // Use the existing isTauriEnv state instead of checking again
      if (
        isTauriEnv &&
        (file.type === "application/pdf" ||
          file.name.endsWith(".pdf") ||
          file.type === "text/plain" ||
          file.name.endsWith(".txt") ||
          file.name.endsWith(".md"))
      ) {
        // Process documents locally using Rust in Tauri
        const { invoke } = await import("@tauri-apps/api/core");

        // Convert file to base64
        const reader = new FileReader();
        const base64Data = await new Promise<string>((resolve, reject) => {
          reader.onload = () => {
            const base64 = (reader.result as string).split(",")[1]; // Remove data:type;base64, prefix
            resolve(base64);
          };
          reader.onerror = reject;
          reader.readAsDataURL(file);
        });

        // Determine file type
        let fileType = file.type;
        if (file.name.endsWith(".pdf")) fileType = "pdf";
        else if (file.name.endsWith(".txt")) fileType = "txt";
        else if (file.name.endsWith(".md")) fileType = "md";

        // Call Rust function to extract content
        const rustResponse = await invoke<RustDocumentResponse>("extract_document_content", {
          fileBase64: base64Data,
          filename: file.name,
          fileType: fileType
        });

        // Convert Rust response to ParsedDocument format
        parsed = {
          document: {
            filename: rustResponse.document.filename,
            text_content: rustResponse.document.text_content
          },
          status: rustResponse.status,
          errors: []
        };
      } else if (
        file.type === "text/plain" ||
        file.name.endsWith(".txt") ||
        file.name.endsWith(".md")
      ) {
        // Process text files locally in browser
        parsed = await processTextFileLocally(file);
      } else {
        // PDF files in browser are not supported without Tauri
        setDocumentError("PDF files can only be processed in the desktop app");
        e.target.value = ""; // Reset input
        return;
        // REMOVED: Cloud API fallback for document processing
        // result = await os.uploadDocumentWithPolling(file);
        // parsed = JSON.parse(result.text) as ParsedDocument;
      }

      // Extract content
      // const content = parsed.document.text_content || "";

      // Create a cleaned version of the parsed document
      const cleanedParsed = {
        ...parsed,
        document: {
          ...parsed.document,
          text_content: parsed.document.text_content
            ? parsed.document.text_content.replace(/!\[Image\]\([^)]+\)/g, "")
            : parsed.document.text_content
        }
      };

      // Always provide a valid original-like payload; Tauri-local paths have no server result
      const originalResponse: DocumentResponse =
        result ??
        ({
          text: JSON.stringify(parsed),
          filename: file.name,
          size: file.size
        } as DocumentResponse);

      setUploadedDocument({
        original: originalResponse,
        parsed: parsed,
        cleanedText: JSON.stringify(cleanedParsed) // Store the cleaned JSON as a string
      });
    } catch (error) {
      console.error("Document upload failed:", error);
      if (error instanceof Error) {
        if (error.message.includes("exceeds maximum limit")) {
          setDocumentError("File too large. Maximum size is 10MB.");
        } else if (error.message.includes("401")) {
          setDocumentError("Authentication required. Please log in to upload documents.");
        } else if (error.message.includes("403")) {
          setDocumentError("Usage limit exceeded. Please upgrade your plan.");
        } else {
          setDocumentError("Failed to process document. Please try again.");
        }
      } else {
        setDocumentError("An unexpected error occurred.");
      }
    } finally {
      setIsUploadingDocument(false);
      if (e.target) e.target.value = "";
    }
  };

  const removeDocument = () => {
    setUploadedDocument(null);
    setDocumentError(null);
  };

  // Audio recording functions
  const startRecording = async () => {
    // Prevent duplicate starts
    if (isRecording || isTranscribing) return;

    try {
      // Check if getUserMedia is available
      if (!navigator.mediaDevices || !navigator.mediaDevices.getUserMedia) {
        setAudioError(
          "Microphone access is blocked. Please check your browser permissions or disable Lockdown Mode for this site (Settings > Safari > Advanced > Lockdown Mode)."
        );
        setTimeout(() => setAudioError(null), 8000); // Longer timeout for this important message
        return;
      }

      const stream = await navigator.mediaDevices.getUserMedia({
        audio: {
          echoCancellation: false, // Disable to reduce processing overhead
          noiseSuppression: true,
          autoGainControl: false, // Disable AGC to prevent audio ducking
          sampleRate: 16000 // Lower sample rate to match output
        }
      });

      streamRef.current = stream;

      // Create RecordRTC instance configured for WAV
      const recorder = new RecordRTC(stream, {
        type: "audio",
        mimeType: "audio/wav",
        recorderType: RecordRTC.StereoAudioRecorder,
        numberOfAudioChannels: 1, // Mono audio for smaller file size
        desiredSampRate: 16000 // 16kHz is good for speech
      });

      recorderRef.current = recorder;
      recorder.startRecording();
      setIsRecording(true);
      setAudioError(null); // Clear any previous errors
    } catch (error) {
      console.error("Failed to start recording:", error);
      const err = error as Error & { name?: string };
      console.error("Error name:", err.name);
      console.error("Error message:", err.message);

      // Handle different error types
      if (err.name === "NotAllowedError" || err.name === "PermissionDeniedError") {
        setAudioError(
          "Microphone access denied. Please enable microphone permissions in Settings > Maple."
        );
      } else if (err.name === "NotFoundError" || err.name === "DevicesNotFoundError") {
        setAudioError("No microphone found. Please check your device.");
      } else if (err.name === "NotReadableError" || err.name === "TrackStartError") {
        setAudioError("Microphone is already in use by another app.");
      } else {
        // Include error details for debugging
        setAudioError(
          `Failed to access microphone: ${err.name || "Unknown error"} - ${err.message || "Please try again"}`
        );
      }

      // Clear error after 5 seconds
      setTimeout(() => setAudioError(null), 5000);
    }
  };

  const stopRecording = (shouldSend: boolean = false) => {
    if (recorderRef.current && isRecording) {
      // Only hide immediately if canceling, keep visible if sending
      if (!shouldSend) {
        setIsRecording(false);
      } else {
        setIsProcessingSend(true); // Show processing state
      }

      recorderRef.current.stopRecording(async () => {
        // Safely get blob (recorder might be null by now)
        const blob = recorderRef.current?.getBlob();

        if (!blob || blob.size === 0) {
          console.error("No audio recorded or empty recording");
          if (shouldSend) {
            setAudioError("No audio was recorded. Please try again.");
            setTimeout(() => setAudioError(null), 5000);
          }
          // Still need to clean up
          if (streamRef.current) {
            streamRef.current.getTracks().forEach((track) => track.stop());
            streamRef.current = null;
          }
          recorderRef.current = null;
          setIsProcessingSend(false);
          setIsRecording(false);
          return;
        }

        // Create a proper WAV file
        const audioFile = new File([blob], "recording.wav", {
          type: "audio/wav"
        });

        if (shouldSend) {
          setIsTranscribing(true);
          try {
            const result = await os.transcribeAudio(audioFile, "whisper-large-v3");

            // Set the transcribed text
            const transcribedText = result.text.trim();

            if (transcribedText) {
              // Directly submit without updating the input field
              const newValue = inputValue ? `${inputValue} ${transcribedText}` : transcribedText;

              if (newValue.trim()) {
                // Wait for onSubmit to complete (in case it returns a Promise for navigation)
                await onSubmit(
                  newValue.trim(),
                  messages.length === 0 ? systemPromptValue.trim() || undefined : undefined,
                  images,
                  uploadedDocument?.cleanedText,
                  uploadedDocument
                    ? {
                        filename: uploadedDocument.parsed.document.filename,
                        fullContent: uploadedDocument.parsed.document.text_content || ""
                      }
                    : undefined,
                  true // sentViaVoice flag
                );

                // Clear the input and other states
                setInputValue("");
                imageUrls.forEach((url) => URL.revokeObjectURL(url));
                setImageUrls(new Map());
                setImages([]);
                setUploadedDocument(null);
                setDocumentError(null);
                setImageError(null);
              }
            }
          } catch (error) {
            console.error("Transcription failed:", error);
            setAudioError("Failed to transcribe audio. Please try again.");
            // Clear error after 5 seconds
            setTimeout(() => setAudioError(null), 5000);
          } finally {
            setIsTranscribing(false);
            setIsProcessingSend(false);
            setIsRecording(false); // Hide overlay after send is complete
          }
        }

        // Clean up
        if (streamRef.current) {
          streamRef.current.getTracks().forEach((track) => track.stop());
          streamRef.current = null;
        }
        recorderRef.current = null;
      });
    }
  };

  const toggleRecording = () => {
    if (isRecording) {
      stopRecording();
    } else {
      startRecording();
    }
  };

  const handleRecordingSend = () => {
    stopRecording(true);
  };

  const handleRecordingCancel = () => {
    if (recorderRef.current && isRecording) {
      setIsRecording(false); // Hide overlay immediately

      recorderRef.current.stopRecording(() => {
        // Clean up without transcribing
        if (streamRef.current) {
          streamRef.current.getTracks().forEach((track) => track.stop());
          streamRef.current = null;
        }
        recorderRef.current = null;
      });
    }
  };

  const [isFocused, setIsFocused] = useState(false);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const systemPromptRef = useRef<HTMLTextAreaElement>(null);
  const lastDraftRef = useRef<string>("");
  const previousChatIdRef = useRef<string | undefined>(undefined);
  const currentInputRef = useRef<string>("");

  // Get the chatId from the current route state
  const router = useRouter();
  const chatId = router.state.matches.find((m) => m.routeId === ChatRoute.id)?.params?.chatId as
    | string
    | undefined;

  const { data: freshBillingStatus } = useQuery({
    queryKey: ["billingStatus"],
    queryFn: async () => {
      const billingService = getBillingService();
      const status = await billingService.getBillingStatus();
      setBillingStatus(status);
      return status;
    }
  });

  // Use the centralized hook for mobile detection directly
  const isMobile = useIsMobile();

  // Check if we're in Tauri environment on component mount
  useEffect(() => {
    import("@tauri-apps/api/core")
      .then((m) => m.isTauri())
      .then(setIsTauriEnv)
      .catch(() => setIsTauriEnv(false));
  }, []);

  // Check if user can use system prompts (paid users only - exclude free plans)
  const canUseSystemPrompt =
    freshBillingStatus && !freshBillingStatus.product_name?.toLowerCase().includes("free");

  // Check if system prompt can be edited (only for new chats)
  const canEditSystemPrompt = canUseSystemPrompt && messages.length === 0;

  // Check if user has access to Pro/Team/Max features (Pro, Max, or Team plan)
  const hasProTeamAccess =
    freshBillingStatus &&
    (freshBillingStatus.product_name?.toLowerCase().includes("pro") ||
      freshBillingStatus.product_name?.toLowerCase().includes("max") ||
      freshBillingStatus.product_name?.toLowerCase().includes("team"));

  // Check if user has access to Starter features (Starter plan and above)
  const hasStarterAccess =
    freshBillingStatus &&
    (freshBillingStatus.product_name?.toLowerCase().includes("starter") ||
      freshBillingStatus.product_name?.toLowerCase().includes("pro") ||
      freshBillingStatus.product_name?.toLowerCase().includes("max") ||
      freshBillingStatus.product_name?.toLowerCase().includes("team"));

  const canUseImages = hasStarterAccess;
  const canUseVoice = hasProTeamAccess;
  const canUseDocuments = hasProTeamAccess;

  const handleSubmit = (e?: React.FormEvent) => {
    e?.preventDefault();

    // Allow submission if there's text input, images, or a document
    const hasContent = inputValue.trim() || images.length > 0 || uploadedDocument;
    if (!hasContent || isSubmitDisabled) return;

    // Clear the drafts when submitting
    if (chatId) {
      try {
        clearDraftMessage(chatId);
        lastDraftRef.current = "";
        currentInputRef.current = "";
      } catch (error) {
        console.error("Failed to clear draft messages:", error);
        // Continue with submission even if draft clearing fails
      }
    }

    // Only pass system prompt if this is the first message
    const isFirstMessage = messages.length === 0;
    onSubmit(
      inputValue.trim(),
      isFirstMessage ? systemPromptValue.trim() || undefined : undefined,
      images,
      uploadedDocument?.cleanedText, // Now contains the full JSON with cleaned text_content
      uploadedDocument
        ? {
            filename: uploadedDocument.parsed.document.filename,
            fullContent: uploadedDocument.parsed.document.text_content || ""
          }
        : undefined
    );
    setInputValue("");

    // Clean up image URLs when clearing images
    imageUrls.forEach((url) => URL.revokeObjectURL(url));
    setImageUrls(new Map());
    setImages([]);

    setUploadedDocument(null);
    setDocumentError(null);
    setImageError(null);

    // Re-focus input after submitting (desktop only)
    if (!isMobile) {
      setTimeout(() => {
        inputRef.current?.focus();
      }, 0);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter") {
      if (isMobile || e.shiftKey || isStreaming) {
        // On mobile, when Shift is pressed, or when streaming, allow newline
        return;
      } else if (isSubmitDisabled || !inputValue.trim()) {
        // Prevent form submission when disabled or empty input
        e.preventDefault();
        return;
      } else {
        // On desktop without Shift and not streaming, submit the form
        e.preventDefault();
        handleSubmit();
      }
    }
  };

  // Auto-resize effect for main input
  useEffect(() => {
    if (inputRef.current) {
      inputRef.current.style.height = "auto";
      inputRef.current.style.height = `${inputRef.current.scrollHeight}px`;
    }
  }, [inputValue]);

  // Auto-resize effect for system prompt
  useEffect(() => {
    if (systemPromptRef.current) {
      systemPromptRef.current.style.height = "auto";
      systemPromptRef.current.style.height = `${systemPromptRef.current.scrollHeight}px`;
    }
  }, [systemPromptValue]);

  // Debounce input for token calculations to avoid lag while typing
  const debouncedInputValue = useDebounce(inputValue, 300);

  // Calculate token usage percentage
  const totalTokens = useMemo(
    () => calculateTotalTokens(messages, debouncedInputValue),
    [messages, debouncedInputValue]
  );
  const tokenLimit = getModelTokenLimit(model);
  const tokenPercentage = (totalTokens / tokenLimit) * 100;
  const isAt99Percent = tokenPercentage >= 99;

  // Update current input ref when input value changes
  useEffect(() => {
    currentInputRef.current = inputValue;
  }, [inputValue]);

  // Handle draft loading and saving only on chat switches
  useEffect(() => {
    // 1. Save drafts from previous chat before switching
    if (previousChatIdRef.current && previousChatIdRef.current !== chatId) {
      const oldChatId = previousChatIdRef.current;

      // Save message draft
      const currentInput = currentInputRef.current.trim();
      if (currentInput !== "") {
        setDraftMessage(oldChatId, currentInput);
      } else {
        clearDraftMessage(oldChatId);
      }
    }

    // 2. Load drafts for new chat
    if (chatId) {
      try {
        // Load message draft
        const draft = draftMessages.get(chatId) || "";
        setInputValue(draft);
        lastDraftRef.current = draft;
        currentInputRef.current = draft;

        // Reset system prompt for new chat
        setSystemPromptValue("");
        setIsSystemPromptExpanded(false);
      } catch (error) {
        console.error("Failed to load draft messages:", error);
        setInputValue("");
        setSystemPromptValue("");
        lastDraftRef.current = "";
        currentInputRef.current = "";
      }
    }

    // 3. Update the previous chat ID
    previousChatIdRef.current = chatId;
  }, [chatId, draftMessages, setDraftMessage, clearDraftMessage]);

  // Determine when the submit button should be disabled
  const isSubmitDisabled =
    (freshBillingStatus !== undefined &&
      (!freshBillingStatus.can_chat ||
        (freshBillingStatus.chats_remaining !== null &&
          freshBillingStatus.chats_remaining <= 0))) ||
    isStreaming ||
    isAt99Percent;

  // Disable the input box only when the user is out of chats or when streaming
  const isInputDisabled =
    (freshBillingStatus !== undefined &&
      (!freshBillingStatus.can_chat ||
        (freshBillingStatus.chats_remaining !== null &&
          freshBillingStatus.chats_remaining <= 0))) ||
    isStreaming;

  // Auto-focus effect - runs on mount, when chat ID changes, and after streaming completes
  useEffect(() => {
    // Skip auto-focus on mobile to prevent keyboard popup
    if (isMobile) {
      return;
    }

    // Skip if user is already focused on an input elsewhere
    if (document.activeElement?.matches("input, textarea")) {
      return;
    }

    // Short delay to ensure DOM is ready
    const timer = setTimeout(() => {
      // Only focus if input isn't disabled
      if (inputRef.current && !isInputDisabled) {
        inputRef.current.focus();
      }
    }, 100);

    return () => clearTimeout(timer);
  }, [chatId, isStreaming, isInputDisabled, isMobile]); // Re-run when chat ID changes, streaming completes, or input state changes

  // Cleanup effect for object URLs
  useEffect(() => {
    return () => {
      // Revoke all object URLs when component unmounts
      imageUrls.forEach((url) => URL.revokeObjectURL(url));
    };
  }, [imageUrls]);

  // Cleanup audio recording on unmount
  useEffect(() => {
    return () => {
      // Stop any active recording and release microphone
      if (streamRef.current) {
        streamRef.current.getTracks().forEach((track) => track.stop());
        streamRef.current = null;
      }
      // Clean up recorder
      if (recorderRef.current && isRecording) {
        recorderRef.current.stopRecording(() => {
          recorderRef.current = null;
        });
      }
    };
  }, []);

  // No longer need token calculation or plan type check since we removed the hard limit
  // Just keeping the TokenWarning component which handles its own calculations
  const placeholderText = (() => {
    if (isAt99Percent) {
      return "Chat is too long to continue.";
    }
    if (billingStatus === null || freshBillingStatus === undefined)
      return "Type your message here...";
    if (freshBillingStatus.can_chat === false) {
      return "You've used up all your messages. Upgrade to continue.";
    }
    return "Type your message here...";
  })();

  return (
    <div className="flex flex-col w-full relative">
      {isRecording && (
        <RecordingOverlay
          isRecording={isRecording}
          isProcessing={isProcessingSend}
          onSend={handleRecordingSend}
          onCancel={handleRecordingCancel}
          isCompact={!startTall}
        />
      )}
      <div className={cn("flex flex-col w-full", isRecording && "invisible")}>
        {/* Simple System Prompt Section - just a gear button and input when expanded */}
        <div className={cn(chatId ? "hidden" : !canEditSystemPrompt ? "invisible mb-2" : "mb-2")}>
          <div className="flex items-center gap-2 mb-1">
            <button
              type="button"
              disabled={!canEditSystemPrompt}
              onClick={() => setIsSystemPromptExpanded(!isSystemPromptExpanded)}
              className="flex items-center gap-1.5 text-xs font-medium transition-colors text-muted-foreground hover:text-foreground cursor-pointer disabled:cursor-default"
              title="System Prompt"
              aria-label="Toggle system prompt"
              aria-expanded={isSystemPromptExpanded}
            >
              <Bot className="size-6" />
              {systemPromptValue.trim() && (
                <div className="size-2 bg-primary rounded-full" title="System prompt active" />
              )}
            </button>
          </div>

          {isSystemPromptExpanded && (
            <textarea
              ref={systemPromptRef}
              value={systemPromptValue}
              onChange={(e) => setSystemPromptValue(e.target.value)}
              placeholder="Enter instructions for the AI (e.g., 'You are a helpful coding assistant...')"
              rows={2}
              className="w-full p-2 text-sm border border-muted-foreground/20 rounded-md bg-muted/50 placeholder:text-muted-foreground/70 focus:outline-none focus:ring-1 focus:ring-ring resize-none transition-colors"
              style={{
                height: "auto",
                resize: "none",
                overflowY: "auto",
                maxHeight: "8rem",
                minHeight: "3rem"
              }}
            />
          )}
        </div>

        <TokenWarning
          chatId={chatId}
          onCompress={onCompress}
          isCompressing={isSummarizing}
          tokenPercentage={tokenPercentage}
        />

        <form
          className={cn(
            "p-2 rounded-lg border border-primary bg-background/80 backdrop-blur-lg focus-within:ring-1 focus-within:ring-ring",
            isInputDisabled && "opacity-50"
          )}
          onSubmit={handleSubmit}
          onClick={(e) => {
            if (isInputDisabled) {
              e.preventDefault();
              return;
            }
            // Don't auto-focus if clicking on a button or interactive element
            const target = e.target as HTMLElement;
            const isInteractiveElement =
              target.tagName === "BUTTON" ||
              target.closest("button") ||
              target.tagName === "INPUT" ||
              target.tagName === "SELECT" ||
              target.closest('[role="button"]') ||
              target.closest('[role="combobox"]');

            if (!isInteractiveElement && !isFocused) {
              inputRef.current?.focus();
            }
          }}
        >
          {(images.length > 0 ||
            uploadedDocument ||
            isUploadingDocument ||
            documentError ||
            imageError ||
            imageConversionError ||
            audioError) && (
            <div className="mb-2 space-y-2">
              {images.length > 0 && (
                <div className="flex gap-2 items-center flex-wrap">
                  {images.map((f, i) => (
                    <div key={i} className="relative">
                      <img
                        src={imageUrls.get(f) || ""}
                        className="w-10 h-10 object-cover rounded-md"
                        alt={`Uploaded image ${i + 1}`}
                      />
                      <button
                        type="button"
                        onClick={() => removeImage(i)}
                        className="absolute -top-1 -right-1 bg-background rounded-full shadow-sm"
                        aria-label={`Remove image ${i + 1}`}
                      >
                        <X className="h-3 w-3" />
                      </button>
                    </div>
                  ))}
                </div>
              )}
              {(imageError || imageConversionError) && (
                <div className="text-xs text-destructive p-2 bg-destructive/10 rounded-md">
                  {imageError || imageConversionError}
                </div>
              )}
              {isUploadingDocument && !uploadedDocument && (
                <div className="flex items-center gap-2 p-2 bg-muted/50 rounded-md animate-in fade-in duration-200">
                  <Loader2 className="h-4 w-4 text-muted-foreground animate-spin" />
                  <span className="text-sm text-muted-foreground">
                    Processing document securely... This may take a minute.
                  </span>
                </div>
              )}
              {uploadedDocument && (
                <div className="flex items-center gap-2 p-2 bg-muted/50 rounded-md">
                  <FileText className="h-4 w-4 text-muted-foreground" />
                  <span className="text-sm truncate flex-1">
                    {uploadedDocument.parsed.document.filename}
                  </span>
                  <button
                    type="button"
                    onClick={removeDocument}
                    className="text-muted-foreground hover:text-foreground"
                    aria-label="Remove document"
                  >
                    <X className="h-3 w-3" />
                  </button>
                </div>
              )}
              {documentError && (
                <div className="text-xs text-destructive p-2 bg-destructive/10 rounded-md">
                  {documentError}
                </div>
              )}
              {audioError && (
                <div className="text-xs text-destructive p-2 bg-destructive/10 rounded-md">
                  {audioError}
                </div>
              )}
            </div>
          )}
          <Label htmlFor="message" className="sr-only">
            Message
          </Label>
          <textarea
            disabled={isInputDisabled}
            ref={inputRef}
            onKeyDown={handleKeyDown}
            onFocus={() => setIsFocused(true)}
            onBlur={() => setIsFocused(false)}
            id="message"
            name="message"
            autoComplete="off"
            placeholder={placeholderText}
            rows={1}
            style={{
              height: "auto",
              resize: "none",
              overflowY: "auto",
              maxHeight: "12rem",
              ...(startTall ? { minHeight: "6rem" } : {})
            }}
            className={cn(
              "flex w-full ring-offset-background bg-background/0",
              "placeholder:text-muted-foreground focus-visible:outline-none",
              "disabled:cursor-not-allowed disabled:opacity-50",
              "!border-0 shadow-none !border-none focus-visible:ring-0 !ring-0",
              billingStatus === null && "animate-pulse"
            )}
            value={inputValue}
            onChange={(e) => setInputValue(e.target.value)}
          />
          <div className="flex items-center pt-0">
            <ModelSelector messages={messages} draftImages={images} />

            {/* Hidden file inputs */}
            <input
              type="file"
              accept="image/jpeg,image/jpg,image/png,image/webp"
              multiple
              ref={fileInputRef}
              onChange={handleAddImages}
              className="hidden"
            />
            <input
              type="file"
              accept=".pdf,.txt,.md,application/pdf,text/plain,text/markdown"
              ref={documentInputRef}
              onChange={handleDocumentUpload}
              className="hidden"
            />

            {/* Image upload button - show for all users */}
            {!uploadedDocument && (
              <Button
                type="button"
                size="sm"
                variant="ghost"
                className={cn("ml-1", !canUseImages && "opacity-50")}
                onClick={() => {
                  if (!canUseImages) {
                    setUpgradeFeature("image");
                    setUpgradeDialogOpen(true);
                  } else {
                    // If not on a vision model, switch to one first
                    if (!supportsVision) {
                      const visionModelId = findFirstVisionModel();
                      if (visionModelId) {
                        setModel(visionModelId);
                      }
                    }
                    fileInputRef.current?.click();
                  }
                }}
                disabled={isInputDisabled}
                aria-label="Upload images"
                data-testid="image-upload-button"
              >
                <Image className="h-4 w-4" />
              </Button>
            )}

            {/* Document upload button - show on all platforms but handle differently */}
            {!uploadedDocument && (
              <Button
                type="button"
                size="sm"
                variant="ghost"
                className={cn("ml-1", (!canUseDocuments || !isTauriEnv) && "opacity-50")}
                onClick={async () => {
                  // On web, show platform dialog
                  if (!isTauriEnv) {
                    setDocumentPlatformDialogOpen(true);
                    return;
                  }
                  // On desktop/mobile, check if user has access
                  if (!canUseDocuments) {
                    setUpgradeFeature("document");
                    setUpgradeDialogOpen(true);
                    return;
                  }
                  // Try using Tauri's native file dialog
                  try {
                    const { open } = await import("@tauri-apps/plugin-dialog");
                    const { platform } = await import("@tauri-apps/plugin-os");
                    const currentPlatform = await platform();

                    let selected;
                    if (currentPlatform === "ios") {
                      // iOS requires MIME types for filters to work
                      selected = await open({
                        multiple: false,
                        filters: [
                          {
                            name: "Documents",
                            extensions: ["application/pdf", "text/plain", "text/markdown"]
                          }
                        ]
                      });
                    } else {
                      // Desktop platforms use file extensions
                      selected = await open({
                        multiple: false,
                        filters: [
                          {
                            name: "Documents",
                            extensions: ["pdf", "txt", "md"]
                          }
                        ]
                      });
                    }

                    if (selected && typeof selected === "string") {
                      // Read the file and convert to File object for processing
                      const fs = await import("@tauri-apps/plugin-fs");
                      const fileContent = await fs.readFile(selected);

                      // Get filename from path
                      const filename = selected.split(/[/\\]/).pop() || "document";

                      // Create a File object from the binary content
                      const file = new File([new Blob([fileContent])], filename, {
                        type: filename.endsWith(".pdf")
                          ? "application/pdf"
                          : filename.endsWith(".md")
                            ? "text/markdown"
                            : "text/plain"
                      });

                      // Process the file using our existing handler
                      const fakeEvent = {
                        target: { files: [file], value: "" },
                        preventDefault: () => {}
                      } as unknown as React.ChangeEvent<HTMLInputElement>;

                      await handleDocumentUpload(fakeEvent);
                    }
                  } catch (error) {
                    console.error("Failed to open file dialog:", error);
                    // Fallback to HTML input
                    documentInputRef.current?.click();
                  }
                }}
                disabled={isInputDisabled}
                aria-label="Upload document"
                data-testid="document-upload-button"
              >
                <FileText className="h-4 w-4" />
              </Button>
            )}

            {/* Microphone button - show if whisper model is available */}
            {hasWhisperModel && (
              <Button
                type="button"
                size="sm"
                variant="ghost"
                className={cn("ml-1", !canUseVoice && "opacity-50")}
                onClick={() => {
                  if (!canUseVoice) {
                    setUpgradeFeature("voice");
                    setUpgradeDialogOpen(true);
                  } else {
                    toggleRecording();
                  }
                }}
                disabled={isTranscribing || isInputDisabled}
                aria-label={isRecording ? "Stop recording" : "Start recording"}
                data-testid="mic-button"
              >
                {isTranscribing ? (
                  <Loader2 className="h-4 w-4 animate-spin" />
                ) : isRecording ? (
                  <Mic className="h-4 w-4 text-orange-500" />
                ) : (
                  <Mic className="h-4 w-4" />
                )}
              </Button>
            )}

            <Button
              type="submit"
              size="sm"
              className="ml-auto gap-1.5"
              disabled={
                (!inputValue.trim() && images.length === 0 && !uploadedDocument) || isSubmitDisabled
              }
              aria-label="Send message"
            >
              <CornerRightUp className="size-3.5" />
            </Button>
          </div>
        </form>

        {/* Upgrade prompt dialog */}
        <UpgradePromptDialog
          open={upgradeDialogOpen}
          onOpenChange={setUpgradeDialogOpen}
          feature={upgradeFeature}
        />

        {/* Document platform dialog for web users */}
        <DocumentPlatformDialog
          open={documentPlatformDialogOpen}
          onOpenChange={setDocumentPlatformDialogOpen}
          hasProAccess={canUseDocuments || false}
        />
      </div>
    </div>
  );
}
