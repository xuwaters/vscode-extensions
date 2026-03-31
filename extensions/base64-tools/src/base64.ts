import { Buffer } from 'node:buffer';

const BASE64_REGEX = /^[A-Za-z0-9+/]*={0,2}$/;
const BASE64URL_REGEX = /^[A-Za-z0-9_-]+$/;
const BASE64URL_PART_REGEX = /^[A-Za-z0-9_-]+$/;

export function encodeBase64(text: string): string {
  return Buffer.from(text, 'utf8').toString('base64');
}

export function decodeBase64(text: string): { ok: true; value: string } | { ok: false; error: string } {
  const trimmed = text.trim();
  if (BASE64_REGEX.test(trimmed)) {
    return { ok: true, value: Buffer.from(trimmed, 'base64').toString('utf8') };
  }
  if (BASE64URL_REGEX.test(trimmed)) {
    return { ok: true, value: decodeBase64Url(trimmed) };
  }
  return { ok: false, error: 'Selection is not valid base64.' };
}

function decodeBase64Url(part: string): string {
  const padded = part.replace(/-/g, '+').replace(/_/g, '/');
  const pad = padded.length % 4;
  const padded2 = pad ? padded + '='.repeat(4 - pad) : padded;
  return Buffer.from(padded2, 'base64').toString('utf8');
}

export function isJwtLike(text: string): boolean {
  const trimmed = text.trim();
  const parts = trimmed.split('.');
  if (parts.length < 2) return false;
  return parts.every(p => p.length == 0 || BASE64URL_PART_REGEX.test(p));
}

export function decodeJwtLike(text: string): { ok: true; value: string } | { ok: false; error: string } {
  const trimmed = text.trim();
  const parts = trimmed.split('.');
  // Decode first parts (header + payload); preserve last part as-is when there are 3+ parts (signature is binary)
  const decodeParts = parts.length >= 3 ? parts.slice(0, -1) : parts;
  const preservedPart = parts.length >= 3 ? parts[parts.length - 1] : null;
  try {
    const decoded = decodeParts.map(part => {
      const raw = decodeBase64Url(part);
      try {
        return JSON.stringify(JSON.parse(raw), null, 2);
      } catch {
        return raw;
      }
    });
    if (preservedPart !== null) {
      decoded.push(preservedPart);
    }
    return { ok: true, value: decoded.join('\n.\n') };
  } catch (e) {
    return { ok: false, error: `Failed to decode JWT-like string: ${e}` };
  }
}
