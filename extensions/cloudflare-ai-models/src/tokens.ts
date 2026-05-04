const CHARS_PER_TOKEN = 3.5;

export function estimateTokens(text: string): number {
  if (!text) return 0;
  return Math.ceil(text.length / CHARS_PER_TOKEN);
}

export function estimateMessageTokens(role: string, parts: ReadonlyArray<unknown>): number {
  let chars = role.length + 4;
  for (const part of parts) {
    chars += charCountOfPart(part);
  }
  return Math.ceil(chars / CHARS_PER_TOKEN);
}

function charCountOfPart(part: unknown): number {
  if (typeof part === 'string') return part.length;
  if (part && typeof part === 'object') {
    if ('value' in part && typeof (part as { value: unknown }).value === 'string') {
      return ((part as { value: string }).value).length;
    }
    if ('content' in part) {
      const c = (part as { content: unknown }).content;
      if (Array.isArray(c)) {
        let sum = 0;
        for (const sub of c) sum += charCountOfPart(sub);
        return sum;
      }
    }
    try {
      return JSON.stringify(part).length;
    } catch {
      return 0;
    }
  }
  return 0;
}
