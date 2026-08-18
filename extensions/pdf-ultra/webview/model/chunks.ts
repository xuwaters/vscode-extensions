/**
 * The byte fallback's receiving end.
 *
 * Most documents never come this way — the page reads them straight off
 * VSCode's resource server. This is for the ones with no such URL, which arrive
 * base64-sliced over the message channel instead.
 *
 * The host cuts on a multiple of three bytes, so each slice is a complete
 * base64 string and decodes on its own. That is what lets this decode as it
 * goes: a 200 MB document has no business existing as one 270-million-character
 * string on the way to becoming a buffer.
 */
export class ChunkBuffer {
  private parts: (Uint8Array | undefined)[] = [];
  private have = 0;
  private expected = 0;

  /** How much of the document has landed, 0 to 1. */
  get progress(): number {
    return this.expected > 0 ? this.have / this.expected : 0;
  }

  /** Throw away a partial transfer — a new one has superseded it. */
  reset(): void {
    this.parts = [];
    this.have = 0;
    this.expected = 0;
  }

  /**
   * Take one slice. Returns the assembled document once the last one lands, and
   * null until then. Throws only if a slice is not base64, which means the
   * transfer is corrupt and no later slice can repair it.
   *
   * A slice that arrives twice is counted once: `total` is authoritative, and
   * completing on a duplicate would assemble a document with a hole in it.
   */
  add(index: number, total: number, data: string): Uint8Array | null {
    if (total <= 0 || index < 0 || index >= total) return null;
    if (total !== this.expected) {
      this.reset();
      this.expected = total;
    }
    if (this.parts[index] === undefined) this.have += 1;
    this.parts[index] = decode(data);
    if (this.have < total) return null;

    const assembled = concat(this.parts as Uint8Array[]);
    this.reset();
    return assembled;
  }
}

function decode(base64: string): Uint8Array {
  const binary = atob(base64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) bytes[i] = binary.charCodeAt(i);
  return bytes;
}

function concat(parts: readonly Uint8Array[]): Uint8Array {
  const total = parts.reduce((sum, part) => sum + part.length, 0);
  const out = new Uint8Array(total);
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.length;
  }
  return out;
}
