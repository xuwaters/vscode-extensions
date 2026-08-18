// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest';
import { ChunkBuffer } from './chunks.js';

/** Base64 for a slice of bytes, the way the host cuts them. */
const slice = (...bytes: number[]): string =>
  btoa(String.fromCharCode(...bytes));

describe('assembling a document from slices', () => {
  it('returns nothing until the last one lands', () => {
    const buffer = new ChunkBuffer();
    expect(buffer.add(0, 2, slice(1, 2, 3))).toBeNull();
    expect(buffer.add(1, 2, slice(4, 5, 6))).toEqual(new Uint8Array([1, 2, 3, 4, 5, 6]));
  });

  it('assembles out-of-order slices in index order', () => {
    const buffer = new ChunkBuffer();
    expect(buffer.add(1, 2, slice(4, 5, 6))).toBeNull();
    expect(buffer.add(0, 2, slice(1, 2, 3))).toEqual(new Uint8Array([1, 2, 3, 4, 5, 6]));
  });

  it('does not complete early on a slice that arrives twice', () => {
    // Completing here would assemble a document with a hole in it.
    const buffer = new ChunkBuffer();
    expect(buffer.add(0, 3, slice(1))).toBeNull();
    expect(buffer.add(0, 3, slice(1))).toBeNull();
    expect(buffer.add(1, 3, slice(2))).toBeNull();
    expect(buffer.add(2, 3, slice(3))).toEqual(new Uint8Array([1, 2, 3]));
  });

  it('reports how much has landed', () => {
    const buffer = new ChunkBuffer();
    expect(buffer.progress).toBe(0);
    buffer.add(0, 4, slice(1));
    expect(buffer.progress).toBe(0.25);
  });

  it('is empty again after delivering, so the next document starts clean', () => {
    const buffer = new ChunkBuffer();
    buffer.add(0, 1, slice(1, 2));
    expect(buffer.progress).toBe(0);
    expect(buffer.add(0, 1, slice(9))).toEqual(new Uint8Array([9]));
  });

  it('starts over when a new transfer of a different length begins', () => {
    const buffer = new ChunkBuffer();
    buffer.add(0, 3, slice(1));
    expect(buffer.add(0, 1, slice(7, 8))).toEqual(new Uint8Array([7, 8]));
  });

  it('ignores a slice index that is not in the transfer', () => {
    const buffer = new ChunkBuffer();
    expect(buffer.add(5, 2, slice(1))).toBeNull();
    expect(buffer.add(-1, 2, slice(1))).toBeNull();
    expect(buffer.add(0, 0, slice(1))).toBeNull();
  });

  it('throws on a slice that is not base64 — no later slice can repair that', () => {
    const buffer = new ChunkBuffer();
    expect(() => buffer.add(0, 1, 'not base64!!')).toThrow();
  });
});
