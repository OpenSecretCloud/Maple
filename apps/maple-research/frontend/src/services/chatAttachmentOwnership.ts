export type AttachmentComposerMutationResult<TComposer> = Readonly<{
  composer: TComposer;
  didMutate: boolean;
}>;

/**
 * Attachment controls are owned by one chat runtime. Once that runtime starts a
 * run, its snapshotted attachments stay immutable until the run finishes.
 */
export function mutateAttachmentComposerWhenIdle<TComposer>(
  runtime: Readonly<{ isGenerating: boolean; composer: TComposer }>,
  updater: (composer: TComposer) => TComposer
): AttachmentComposerMutationResult<TComposer> {
  if (runtime.isGenerating) {
    return { composer: runtime.composer, didMutate: false };
  }

  return { composer: updater(runtime.composer), didMutate: true };
}

/**
 * An in-flight document extraction owns its destination runtime until its
 * completion callback clears the processing flag. Adopting that destination
 * into another active run would fence the callback and leave the composer
 * permanently busy.
 */
export function canAdoptAttachmentDestination(
  destination: Readonly<{ composer: Readonly<{ isProcessingDocument: boolean }> }> | undefined
): boolean {
  return !destination?.composer.isProcessingDocument;
}

export type RestoredImageUrlPlan<TFile> = Readonly<{
  imageUrls: Map<TFile, string>;
  createdUrls: string[];
  displacedUrls: string[];
}>;

/**
 * Reuse URLs that still belong to the snapshotted files, create only missing
 * URLs, and report every replaced URL so the caller can revoke it after the
 * owning runtime update succeeds.
 */
export function planRestoredImageUrls<TFile>(
  files: readonly TFile[],
  currentUrls: ReadonlyMap<TFile, string>,
  createObjectUrl: (file: TFile) => string
): RestoredImageUrlPlan<TFile> {
  const imageUrls = new Map<TFile, string>();
  const createdUrls: string[] = [];

  for (const file of files) {
    const existingUrl = currentUrls.get(file);
    if (existingUrl) {
      imageUrls.set(file, existingUrl);
      continue;
    }

    const createdUrl = createObjectUrl(file);
    createdUrls.push(createdUrl);
    imageUrls.set(file, createdUrl);
  }

  const retainedUrls = new Set(imageUrls.values());
  const displacedUrls = Array.from(new Set(currentUrls.values())).filter(
    (url) => !retainedUrls.has(url)
  );

  return { imageUrls, createdUrls, displacedUrls };
}
