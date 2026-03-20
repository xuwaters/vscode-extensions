import { Buffer } from 'node:buffer';

const BASE64_REGEX = /^[A-Za-z0-9+/]*={0,2}$/;

export function encodeBase64(text: string): string {
  return Buffer.from(text, 'utf8').toString('base64');
}

export function decodeBase64(text: string): { ok: true; value: string } | { ok: false; error: string } {
  const trimmed = text.trim();
  if (!BASE64_REGEX.test(trimmed)) {
    return { ok: false, error: 'Selection is not valid base64.' };
  }
  return { ok: true, value: Buffer.from(trimmed, 'base64').toString('utf8') };
}
