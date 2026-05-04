import { describe, expect, it } from 'vitest';
import { ToolCallAccumulator, tryParseJson } from './accumulator.js';

describe('ToolCallAccumulator', () => {
  it('reassembles a single streamed tool call', () => {
    const acc = new ToolCallAccumulator();
    acc.ingest([{ index: 0, id: 'c1', type: 'function', function: { name: 'lookup', arguments: '{"q":' } }]);
    acc.ingest([{ index: 0, function: { arguments: '"hi"}' } }]);
    const calls = acc.finalize();
    expect(calls).toEqual([{ callId: 'c1', name: 'lookup', arguments: '{"q":"hi"}' }]);
  });

  it('reassembles parallel tool calls keyed by index', () => {
    const acc = new ToolCallAccumulator();
    acc.ingest([
      { index: 0, id: 'a', function: { name: 'one' } },
      { index: 1, id: 'b', function: { name: 'two' } },
    ]);
    acc.ingest([
      { index: 0, function: { arguments: '{}' } },
      { index: 1, function: { arguments: '{"x":1}' } },
    ]);
    const calls = acc.finalize();
    expect(calls).toHaveLength(2);
    expect(calls.find(c => c.callId === 'a')?.arguments).toBe('{}');
    expect(calls.find(c => c.callId === 'b')?.arguments).toBe('{"x":1}');
  });

  it('finalize is idempotent (clears state)', () => {
    const acc = new ToolCallAccumulator();
    acc.ingest([{ index: 0, id: 'c1', function: { name: 'n', arguments: '{}' } }]);
    expect(acc.finalize()).toHaveLength(1);
    expect(acc.finalize()).toHaveLength(0);
  });

  it('drops entries with no name', () => {
    const acc = new ToolCallAccumulator();
    acc.ingest([{ index: 0, function: { arguments: 'noisy' } }]);
    expect(acc.finalize()).toEqual([]);
  });

  it('handles undefined input', () => {
    const acc = new ToolCallAccumulator();
    acc.ingest(undefined);
    expect(acc.finalize()).toEqual([]);
  });
});

describe('tryParseJson', () => {
  it('parses valid json', () => {
    expect(tryParseJson('{"a":1}')).toEqual({ a: 1 });
  });
  it('returns undefined on bad json', () => {
    expect(tryParseJson('{not json')).toBeUndefined();
  });
});
