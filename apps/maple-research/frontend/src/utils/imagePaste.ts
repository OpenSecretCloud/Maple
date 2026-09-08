const SUPPORTED_IMAGE_TYPES = ["image/png", "image/jpeg", "image/jpg", "image/webp"] as const;

interface ClipboardEventItemLike {
  type: string;
  getAsFile(): File | null;
}

interface AsyncClipboardItemLike {
  readonly types: readonly string[];
  getType(type: string): Promise<Blob>;
}

type ClipboardReader = () => Promise<readonly AsyncClipboardItemLike[]> | undefined;

interface LinuxTauriClipboardFallbackOptions {
  eventItemTypes: readonly string[];
  isTauri: boolean;
  isLinux: boolean;
  readClipboard?: ClipboardReader;
}

export function getImageFilesFromClipboardItems(items: ArrayLike<ClipboardEventItemLike>): File[] {
  const imageFiles: File[] = [];

  for (let index = 0; index < items.length; index++) {
    const item = items[index];
    if (!item.type.startsWith("image/")) continue;

    const file = item.getAsFile();
    if (file) imageFiles.push(file);
  }

  return imageFiles;
}

/**
 * Starts the async clipboard read synchronously so WebKit can associate it with
 * the paste gesture. Returning undefined means the fallback was not attempted.
 */
export function maybeReadLinuxTauriClipboardImages({
  eventItemTypes,
  isTauri,
  isLinux,
  readClipboard
}: LinuxTauriClipboardFallbackOptions): Promise<File[]> | undefined {
  const normalizedEventTypes = eventItemTypes.map((type) => type.toLowerCase());
  const isEmptyImagePaste = normalizedEventTypes.length === 0;
  const isHtmlOnlyImagePaste =
    normalizedEventTypes.length === 1 && normalizedEventTypes[0] === "text/html";

  if ((!isEmptyImagePaste && !isHtmlOnlyImagePaste) || !isTauri || !isLinux || !readClipboard) {
    return undefined;
  }

  let clipboardItemsPromise: ReturnType<ClipboardReader>;
  try {
    clipboardItemsPromise = readClipboard();
  } catch {
    return Promise.resolve([]);
  }

  if (!clipboardItemsPromise) return undefined;

  return clipboardItemsPromise
    .then(async (clipboardItems) => {
      const imageFiles: File[] = [];

      for (const item of clipboardItems) {
        const imageType = SUPPORTED_IMAGE_TYPES.find((type) => item.types.includes(type));
        if (!imageType) continue;

        try {
          const blob = await item.getType(imageType);
          const extension =
            imageType === "image/jpeg" || imageType === "image/jpg"
              ? "jpg"
              : imageType.split("/")[1];
          imageFiles.push(
            new File([blob], `pasted-image.${extension}`, {
              type: blob.type || imageType
            })
          );
        } catch {
          // A clipboard item can disappear or become unreadable before getType resolves.
        }
      }

      return imageFiles;
    })
    .catch(() => []);
}
