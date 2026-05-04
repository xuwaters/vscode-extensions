export function mergeHeaders(
  ...layers: ReadonlyArray<Record<string, string> | undefined>
): Record<string, string> {
  const out: Record<string, string> = {};
  for (const layer of layers) {
    if (!layer) continue;
    for (const [k, v] of Object.entries(layer)) {
      if (isReservedHeader(k)) continue;
      out[k] = v;
    }
  }
  return out;
}

const RESERVED_HEADERS = new Set([
  'authorization',
  'content-type',
  'content-length',
  'host',
  'connection',
]);

function isReservedHeader(name: string): boolean {
  return RESERVED_HEADERS.has(name.toLowerCase());
}

export interface ModelUrlSource {
  url?: string;
}

export function effectiveModelUrl(model: ModelUrlSource, providerUrl: string): string {
  return (model.url && model.url.trim()) || providerUrl;
}
