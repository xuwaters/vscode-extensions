export interface SSEEvent {
  data: string;
  event?: string;
}

export async function* parseSSE(
  stream: AsyncIterable<Uint8Array> | NodeJS.ReadableStream,
): AsyncGenerator<SSEEvent> {
  const decoder = new TextDecoder('utf-8');
  let buffer = '';

  for await (const chunk of stream as AsyncIterable<Uint8Array | Buffer>) {
    const bytes = chunk instanceof Uint8Array ? chunk : new Uint8Array(chunk);
    buffer += decoder.decode(bytes, { stream: true });
    const parts = buffer.split(/\r?\n\r?\n/);
    buffer = parts.pop() ?? '';
    for (const block of parts) {
      const evt = parseEventBlock(block);
      if (evt) yield evt;
    }
  }
  buffer += decoder.decode();
  if (buffer.length > 0) {
    const evt = parseEventBlock(buffer);
    if (evt) yield evt;
  }
}

function parseEventBlock(block: string): SSEEvent | undefined {
  const lines = block.split(/\r?\n/);
  let event: string | undefined;
  const dataParts: string[] = [];
  for (const raw of lines) {
    if (!raw || raw.startsWith(':')) continue;
    const idx = raw.indexOf(':');
    const field = idx === -1 ? raw : raw.slice(0, idx);
    let value = idx === -1 ? '' : raw.slice(idx + 1);
    if (value.startsWith(' ')) value = value.slice(1);
    if (field === 'data') {
      dataParts.push(value);
    } else if (field === 'event') {
      event = value;
    }
  }
  if (dataParts.length === 0) return undefined;
  return { data: dataParts.join('\n'), event };
}

export function isSSEDone(evt: SSEEvent): boolean {
  return evt.data.trim() === '[DONE]';
}
