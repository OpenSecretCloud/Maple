import {
  MIN_ENCRYPTED_RECORD_BYTES,
  TransportV2ProtocolError,
  concatBytes,
  decodeCanonicalBase64
} from "./encoding";
import { TRANSPORT_V2_LIMITS, type TransportV2StreamRecord, parseStreamRecord } from "./envelope";

// Stream records are structurally bounded far below the generic 50 MiB
// envelope limit: 64 KiB of decoded headers/chunk bytes plus JSON/base64
// framing. This ceiling bounds partial-carrier buffering before decryption.
const MAX_OUTER_STREAM_FRAME_BYTES = 256 * 1024;
const MAX_LOGICAL_STREAM_BYTES = TRANSPORT_V2_LIMITS.responseLogicalBodyBytes;
const FRAME_PREFIX = new TextEncoder().encode("data: ");

export type DecryptStreamRecord = (encrypted: Uint8Array, sequence: number) => Uint8Array;
export type ReleaseStreamContext = () => void;
export type ReserveStreamChunk = () => void;

export class TransportV2StreamDecoder {
  readonly requestId: string;

  #buffer = new Uint8Array(0);
  #decrypt: DecryptStreamRecord;
  #expectedSequence = 0;
  #started = false;
  #terminal = false;
  #failed = false;
  #logicalBytes = 0;
  #maxLogicalBytes: number;
  #releaseContext: ReleaseStreamContext;
  #reserveChunk: ReserveStreamChunk;
  #released = false;

  constructor(
    requestId: string,
    decrypt: DecryptStreamRecord,
    maxLogicalBytes = MAX_LOGICAL_STREAM_BYTES,
    releaseContext: ReleaseStreamContext = () => {},
    reserveChunk: ReserveStreamChunk = () => {}
  ) {
    if (
      !Number.isSafeInteger(maxLogicalBytes) ||
      maxLogicalBytes < 0 ||
      maxLogicalBytes > MAX_LOGICAL_STREAM_BYTES
    ) {
      throw new TransportV2ProtocolError("Transport v2 logical stream limit is invalid.");
    }
    this.requestId = requestId;
    this.#decrypt = decrypt;
    this.#maxLogicalBytes = maxLogicalBytes;
    this.#releaseContext = releaseContext;
    this.#reserveChunk = reserveChunk;
  }

  push(chunk: Uint8Array): TransportV2StreamRecord[] {
    if (this.#failed) {
      throw new TransportV2ProtocolError("Transport v2 stream decoder has failed closed.");
    }
    if (chunk.length === 0) return [];
    if (this.#terminal) {
      return this.#fail("Transport v2 stream contains data after its terminal record.");
    }
    for (const byte of chunk) {
      if (byte === 0x0d || byte > 0x7f) {
        return this.#fail("Transport v2 stream carrier contains invalid bytes.");
      }
    }
    this.#buffer = concatBytes(this.#buffer, chunk);

    const records: TransportV2StreamRecord[] = [];
    while (true) {
      const boundary = findFrameBoundary(this.#buffer);
      if (boundary < 0) {
        if (this.#buffer.length > MAX_OUTER_STREAM_FRAME_BYTES) {
          return this.#fail("Transport v2 stream carrier frame exceeds its size limit.");
        }
        break;
      }
      if (boundary > MAX_OUTER_STREAM_FRAME_BYTES) {
        return this.#fail("Transport v2 stream carrier frame exceeds its size limit.");
      }
      const frame = this.#buffer.slice(0, boundary);
      this.#buffer = this.#buffer.slice(boundary + 2);
      try {
        records.push(this.#decodeFrame(frame));
      } catch (error) {
        this.#failed = true;
        this.#buffer.fill(0);
        this.#buffer = new Uint8Array(0);
        this.#release();
        if (error instanceof TransportV2ProtocolError) throw error;
        throw new TransportV2ProtocolError("Transport v2 stream decoding failed.");
      }
      if (this.#terminal && this.#buffer.length > 0) {
        return this.#fail("Transport v2 stream contains data after its terminal record.");
      }
    }
    return records;
  }

  finish(): void {
    if (this.#failed) {
      throw new TransportV2ProtocolError("Transport v2 stream decoder has failed closed.");
    }
    if (this.#buffer.length !== 0) {
      this.#failed = true;
      this.#buffer.fill(0);
      this.#buffer = new Uint8Array(0);
      this.#release();
      throw new TransportV2ProtocolError("Transport v2 stream ended with a partial carrier frame.");
    }
    if (!this.#terminal) {
      this.#failed = true;
      this.#release();
      throw new TransportV2ProtocolError(
        "Transport v2 stream ended without an authenticated terminal record."
      );
    }
    this.#release();
  }

  get isTerminal(): boolean {
    return this.#terminal;
  }

  dispose(): void {
    if (this.#terminal || this.#failed) {
      this.#release();
      return;
    }
    this.#failed = true;
    this.#buffer.fill(0);
    this.#buffer = new Uint8Array(0);
    this.#release();
  }

  #decodeFrame(frame: Uint8Array): TransportV2StreamRecord {
    if (
      frame.length <= FRAME_PREFIX.length ||
      !FRAME_PREFIX.every((byte, index) => frame[index] === byte) ||
      frame.subarray(FRAME_PREFIX.length).includes(0x0a)
    ) {
      throw new TransportV2ProtocolError("Transport v2 stream carrier framing is invalid.");
    }
    const encoded = new TextDecoder("ascii", { fatal: true }).decode(
      frame.subarray(FRAME_PREFIX.length)
    );
    const encrypted = decodeCanonicalBase64(
      encoded,
      TRANSPORT_V2_LIMITS.responseEnvelopeBytes + MIN_ENCRYPTED_RECORD_BYTES
    );
    let plaintext: Uint8Array | undefined;
    try {
      plaintext = this.#decrypt(encrypted, this.#expectedSequence);
      const record = parseStreamRecord(plaintext);
      if (record.requestId !== this.requestId || record.sequence !== this.#expectedSequence) {
        throw new TransportV2ProtocolError("Transport v2 stream record binding is invalid.");
      }
      if (record.kind === "chunk") {
        // Start and terminal capacity was reserved before the request could be
        // emitted. Every authenticated, bound Chunk permanently charges an
        // additional slot while leaving that terminal reservation unavailable
        // to application bytes, even when later state/size checks reject it.
        this.#reserveChunk();
      }
      if (!this.#started) {
        if (record.kind !== "start") {
          throw new TransportV2ProtocolError("Transport v2 stream does not begin with Start.");
        }
        this.#started = true;
      } else if (record.kind === "start") {
        throw new TransportV2ProtocolError("Transport v2 stream contains more than one Start.");
      }
      if (record.kind === "chunk") {
        const nextLogicalBytes = this.#logicalBytes + record.body.length;
        if (!Number.isSafeInteger(nextLogicalBytes) || nextLogicalBytes > this.#maxLogicalBytes) {
          record.body.fill(0);
          throw new TransportV2ProtocolError("Transport v2 logical stream exceeds its size limit.");
        }
        this.#logicalBytes = nextLogicalBytes;
      }
      if (record.kind === "end" || record.kind === "error") {
        this.#terminal = true;
        this.#release();
      }
      this.#expectedSequence += 1;
      if (!Number.isSafeInteger(this.#expectedSequence)) {
        throw new TransportV2ProtocolError("Transport v2 stream sequence is exhausted.");
      }
      return record;
    } finally {
      encrypted.fill(0);
      plaintext?.fill(0);
    }
  }

  #fail(message: string): never {
    this.#failed = true;
    this.#buffer.fill(0);
    this.#buffer = new Uint8Array(0);
    this.#release();
    throw new TransportV2ProtocolError(message);
  }

  #release(): void {
    if (this.#released) return;
    this.#released = true;
    this.#releaseContext();
    this.#releaseContext = () => {};
  }
}

function findFrameBoundary(buffer: Uint8Array): number {
  for (let index = 0; index + 1 < buffer.length; index += 1) {
    if (buffer[index] === 0x0a && buffer[index + 1] === 0x0a) return index;
  }
  return -1;
}
