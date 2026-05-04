import { describe, expect, it, vi } from 'vitest';

vi.mock('vscode', () => {
  class TextPart {
    constructor(public value: string) {}
  }
  class ToolCallPart {
    constructor(public callId: string, public name: string, public input: unknown) {}
  }
  class ToolResultPart {
    constructor(public callId: string, public content: unknown[]) {}
  }
  class DataPart {
    constructor(public mimeType: string, public data: Uint8Array) {}
  }
  return {
    LanguageModelChatMessageRole: { User: 1, Assistant: 2 },
    LanguageModelChatToolMode: { Auto: 1, Required: 2 },
    LanguageModelTextPart: TextPart,
    LanguageModelToolCallPart: ToolCallPart,
    LanguageModelToolResultPart: ToolResultPart,
    LanguageModelDataPart: DataPart,
  };
});

import * as vscode from 'vscode';
import { buildRequestBody } from './openai.js';

const TextPart = vscode.LanguageModelTextPart as unknown as new (value: string) => unknown;
const ToolCallPart = vscode.LanguageModelToolCallPart as unknown as new (
  callId: string,
  name: string,
  input: unknown,
) => unknown;
const ToolResultPart = vscode.LanguageModelToolResultPart as unknown as new (
  callId: string,
  content: unknown[],
) => unknown;

const ASSISTANT = 2;
const USER = 1;

describe('buildRequestBody', () => {
  it('passes simple text user message through', () => {
    const body = buildRequestBody(
      'm1',
      [{ role: USER, content: [new TextPart('hi')] } as never],
      {} as never,
      false,
    );
    expect(body.messages).toEqual([{ role: 'user', content: 'hi' }]);
    expect(body.stream).toBe(true);
    expect(body.tools).toBeUndefined();
  });

  it('serializes assistant tool calls and content together', () => {
    const body = buildRequestBody(
      'm1',
      [
        {
          role: ASSISTANT,
          content: [
            new TextPart('let me check'),
            new ToolCallPart('c1', 'lookup', { q: 'hi' }),
          ],
        } as never,
      ],
      {} as never,
      true,
    );
    expect(body.messages).toEqual([
      {
        role: 'assistant',
        content: 'let me check',
        tool_calls: [
          { id: 'c1', type: 'function', function: { name: 'lookup', arguments: '{"q":"hi"}' } },
        ],
      },
    ]);
  });

  it('expands parallel tool results into one tool message per call', () => {
    const body = buildRequestBody(
      'm1',
      [
        {
          role: USER,
          content: [
            new ToolResultPart('c1', [new TextPart('ok-1')]),
            new ToolResultPart('c2', [new TextPart('ok-2')]),
          ],
        } as never,
      ],
      {} as never,
      true,
    );
    expect(body.messages).toEqual([
      { role: 'tool', tool_call_id: 'c1', content: 'ok-1' },
      { role: 'tool', tool_call_id: 'c2', content: 'ok-2' },
    ]);
  });

  it('emits tool messages then a trailing user message when text follows results', () => {
    const body = buildRequestBody(
      'm1',
      [
        {
          role: USER,
          content: [
            new ToolResultPart('c1', [new TextPart('ok-1')]),
            new ToolResultPart('c2', [new TextPart('ok-2')]),
            new TextPart('now summarize'),
          ],
        } as never,
      ],
      {} as never,
      true,
    );
    expect(body.messages).toEqual([
      { role: 'tool', tool_call_id: 'c1', content: 'ok-1' },
      { role: 'tool', tool_call_id: 'c2', content: 'ok-2' },
      { role: 'user', content: 'now summarize' },
    ]);
  });

  it('round-trips a parallel-call turn (assistant tool_calls → two tool replies)', () => {
    const body = buildRequestBody(
      'm1',
      [
        { role: USER, content: [new TextPart('search both')] } as never,
        {
          role: ASSISTANT,
          content: [
            new ToolCallPart('a', 'one', {}),
            new ToolCallPart('b', 'two', { x: 1 }),
          ],
        } as never,
        {
          role: USER,
          content: [
            new ToolResultPart('a', [new TextPart('result-a')]),
            new ToolResultPart('b', [new TextPart('result-b')]),
          ],
        } as never,
      ],
      {} as never,
      true,
    );
    expect(body.messages.map(m => m.role)).toEqual(['user', 'assistant', 'tool', 'tool']);
    expect(body.messages[1]!.tool_calls).toHaveLength(2);
    expect(body.messages[2]).toMatchObject({ role: 'tool', tool_call_id: 'a', content: 'result-a' });
    expect(body.messages[3]).toMatchObject({ role: 'tool', tool_call_id: 'b', content: 'result-b' });
  });

  it('attaches tools and tool_choice when toolCalling is enabled', () => {
    const body = buildRequestBody(
      'm1',
      [{ role: USER, content: [new TextPart('hi')] } as never],
      {
        tools: [
          {
            name: 'search',
            description: 'find things',
            inputSchema: { type: 'object', properties: { q: { type: 'string' } } },
          },
        ],
        toolMode: 2,
      } as never,
      true,
    );
    expect(body.tools).toEqual([
      {
        type: 'function',
        function: {
          name: 'search',
          description: 'find things',
          parameters: { type: 'object', properties: { q: { type: 'string' } } },
        },
      },
    ]);
    expect(body.tool_choice).toBe('required');
  });

  it('omits tools when toolCalling is disabled even if options.tools provided', () => {
    const body = buildRequestBody(
      'm1',
      [{ role: USER, content: [new TextPart('hi')] } as never],
      { tools: [{ name: 'x', description: '', inputSchema: {} }] } as never,
      false,
    );
    expect(body.tools).toBeUndefined();
    expect(body.tool_choice).toBeUndefined();
  });

  it('emits an empty assistant content placeholder when only tool_calls', () => {
    const body = buildRequestBody(
      'm1',
      [
        {
          role: ASSISTANT,
          content: [new ToolCallPart('c1', 'fn', {})],
        } as never,
      ],
      {} as never,
      true,
    );
    expect(body.messages).toEqual([
      {
        role: 'assistant',
        tool_calls: [{ id: 'c1', type: 'function', function: { name: 'fn', arguments: '{}' } }],
      },
    ]);
  });
});
