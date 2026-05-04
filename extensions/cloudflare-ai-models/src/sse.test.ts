import { describe, expect, it } from 'vitest';
import { isSSEDone, parseSSE, type SSEEvent } from './sse.js';

async function* fromChunks(chunks: string[]): AsyncGenerator<Uint8Array> {
  for (const c of chunks) yield new TextEncoder().encode(c);
}

async function collect(stream: AsyncIterable<SSEEvent>): Promise<SSEEvent[]> {
  const out: SSEEvent[] = [];
  for await (const e of stream) out.push(e);
  return out;
}

describe('parseSSE', () => {
  it('parses a single event', async () => {
    const events = await collect(parseSSE(fromChunks(['data: hello\n\n'])));
    expect(events).toEqual([{ data: 'hello', event: undefined }]);
  });

  it('parses multiple events split across chunk boundaries', async () => {
    const events = await collect(
      parseSSE(fromChunks(['data: a\n\ndata: ', 'b\n\ndata: c', '\n\n'])),
    );
    expect(events.map(e => e.data)).toEqual(['a', 'b', 'c']);
  });

  it('joins multiple data lines with a newline', async () => {
    const events = await collect(parseSSE(fromChunks(['data: a\ndata: b\n\n'])));
    expect(events[0]?.data).toBe('a\nb');
  });

  it('skips comments (lines beginning with colon)', async () => {
    const events = await collect(
      parseSSE(fromChunks([': keepalive\n\ndata: real\n\n'])),
    );
    expect(events.map(e => e.data)).toEqual(['real']);
  });

  it('captures the event field', async () => {
    const events = await collect(parseSSE(fromChunks(['event: ping\ndata: {}\n\n'])));
    expect(events[0]).toEqual({ data: '{}', event: 'ping' });
  });

  it('handles CRLF line endings', async () => {
    const events = await collect(parseSSE(fromChunks(['data: a\r\n\r\ndata: b\r\n\r\n'])));
    expect(events.map(e => e.data)).toEqual(['a', 'b']);
  });

  it('flushes a trailing event without a final blank line', async () => {
    const events = await collect(parseSSE(fromChunks(['data: a\n\ndata: b'])));
    expect(events.map(e => e.data)).toEqual(['a', 'b']);
  });

  it('detects [DONE]', () => {
    expect(isSSEDone({ data: '[DONE]' })).toBe(true);
    expect(isSSEDone({ data: '  [DONE]  ' })).toBe(true);
    expect(isSSEDone({ data: '{}' })).toBe(false);
  });
});
