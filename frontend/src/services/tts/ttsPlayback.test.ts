import { describe, expect, test } from "bun:test";
import { prepareAndScheduleTTSChunks } from "./ttsPlayback";

describe("prepareAndScheduleTTSChunks", () => {
  test("progressive playback schedules each chunk as soon as it is ready", async () => {
    const events: string[] = [];

    const completed = await prepareAndScheduleTTSChunks({
      chunkCount: 2,
      prebufferBeforePlayback: false,
      prepareChunk: async (chunkIndex) => {
        events.push(`prepare ${chunkIndex}`);
        return chunkIndex;
      },
      scheduleChunk: (chunkIndex) => {
        events.push(`schedule ${chunkIndex}`);
      },
      isActive: () => true
    });

    expect(completed).toBe(true);
    expect(events).toEqual(["prepare 0", "schedule 0", "prepare 1", "schedule 1"]);
  });

  test("buffered playback prepares every chunk before scheduling any audio", async () => {
    const events: string[] = [];

    const completed = await prepareAndScheduleTTSChunks({
      chunkCount: 3,
      prebufferBeforePlayback: true,
      prepareChunk: async (chunkIndex) => {
        events.push(`prepare ${chunkIndex}`);
        return chunkIndex === 1 ? null : chunkIndex;
      },
      beforeBufferedSchedule: async () => {
        events.push("resume");
      },
      scheduleChunk: (chunkIndex) => {
        events.push(`schedule ${chunkIndex}`);
      },
      isActive: () => true
    });

    expect(completed).toBe(true);
    expect(events).toEqual([
      "prepare 0",
      "prepare 1",
      "prepare 2",
      "resume",
      "schedule 0",
      "schedule 2"
    ]);
  });

  test("cancellation during buffered preparation schedules nothing", async () => {
    const scheduledChunks: number[] = [];
    let active = true;

    const completed = await prepareAndScheduleTTSChunks({
      chunkCount: 3,
      prebufferBeforePlayback: true,
      prepareChunk: async (chunkIndex) => {
        if (chunkIndex === 1) {
          active = false;
        }
        return chunkIndex;
      },
      scheduleChunk: (chunkIndex) => {
        scheduledChunks.push(chunkIndex);
      },
      isActive: () => active
    });

    expect(completed).toBe(false);
    expect(scheduledChunks).toEqual([]);
  });

  test("a later preparation error schedules nothing in buffered mode", async () => {
    const scheduledChunks: number[] = [];

    const playback = prepareAndScheduleTTSChunks({
      chunkCount: 3,
      prebufferBeforePlayback: true,
      prepareChunk: async (chunkIndex) => {
        if (chunkIndex === 1) {
          throw new Error("inference failed");
        }
        return chunkIndex;
      },
      scheduleChunk: (chunkIndex) => {
        scheduledChunks.push(chunkIndex);
      },
      isActive: () => true
    });

    await expect(playback).rejects.toThrow("inference failed");
    expect(scheduledChunks).toEqual([]);
  });
});
