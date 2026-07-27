import { describe, expect, test } from "bun:test";
import {
  canAdoptAttachmentDestination,
  mutateAttachmentComposerWhenIdle,
  planRestoredImageUrls
} from "./chatAttachmentOwnership";

describe("chat attachment ownership", () => {
  test("blocks attachment mutation only for the runtime with an active run", () => {
    const runningComposer = { attachments: ["a.png"] };
    const idleComposer = { attachments: ["b.png"] };

    const runningResult = mutateAttachmentComposerWhenIdle(
      { isGenerating: true, composer: runningComposer },
      (composer) => ({ attachments: [...composer.attachments, "follow-up.png"] })
    );
    const idleResult = mutateAttachmentComposerWhenIdle(
      { isGenerating: false, composer: idleComposer },
      (composer) => ({ attachments: [...composer.attachments, "follow-up.png"] })
    );

    expect(runningResult).toEqual({ composer: runningComposer, didMutate: false });
    expect(runningResult.composer).toBe(runningComposer);
    expect(idleResult).toEqual({
      composer: { attachments: ["b.png", "follow-up.png"] },
      didMutate: true
    });
  });

  test("restoration reuses owned URLs and identifies replaced URLs for revocation", () => {
    const retainedFile = { name: "retained.png" };
    const restoredFile = { name: "restored.png" };
    const displacedFile = { name: "displaced.png" };
    const createdFor: string[] = [];

    const plan = planRestoredImageUrls(
      [retainedFile, restoredFile],
      new Map([
        [retainedFile, "blob:retained"],
        [displacedFile, "blob:displaced"]
      ]),
      (file) => {
        createdFor.push(file.name);
        return `blob:created-${file.name}`;
      }
    );

    expect(plan.imageUrls).toEqual(
      new Map([
        [retainedFile, "blob:retained"],
        [restoredFile, "blob:created-restored.png"]
      ])
    );
    expect(createdFor).toEqual(["restored.png"]);
    expect(plan.createdUrls).toEqual(["blob:created-restored.png"]);
    expect(plan.displacedUrls).toEqual(["blob:displaced"]);
  });

  test("rejects adoption while the destination owns document processing", () => {
    expect(canAdoptAttachmentDestination({ composer: { isProcessingDocument: true } })).toBe(false);
    expect(canAdoptAttachmentDestination({ composer: { isProcessingDocument: false } })).toBe(true);
    expect(canAdoptAttachmentDestination(undefined)).toBe(true);
  });
});
