export interface ChatCompletionMessage {
  role: 'system' | 'user' | 'assistant' | 'tool';
  content?: string | null;
  name?: string;
  tool_call_id?: string;
  tool_calls?: Array<{
    id: string;
    type: 'function';
    function: { name: string; arguments: string };
  }>;
}

export interface ChatCompletionTool {
  type: 'function';
  function: {
    name: string;
    description?: string;
    parameters?: object;
  };
}

export interface ChatCompletionRequest {
  model: string;
  messages: ChatCompletionMessage[];
  stream: true;
  tools?: ChatCompletionTool[];
  tool_choice?: 'auto' | 'required' | 'none' | { type: 'function'; function: { name: string } };
  [key: string]: unknown;
}

export interface ToolCallDelta {
  index?: number;
  id?: string;
  type?: 'function';
  function?: { name?: string; arguments?: string };
}

export interface AssembledToolCall {
  callId: string;
  name: string;
  arguments: string;
}

export class ToolCallAccumulator {
  private byIndex = new Map<number, AssembledToolCall>();

  ingest(deltas: ToolCallDelta[] | undefined): void {
    if (!deltas) return;
    for (const d of deltas) {
      const idx = d.index ?? 0;
      const existing = this.byIndex.get(idx) ?? { callId: '', name: '', arguments: '' };
      if (d.id) existing.callId = d.id;
      if (d.function?.name) existing.name = d.function.name;
      if (d.function?.arguments) existing.arguments += d.function.arguments;
      this.byIndex.set(idx, existing);
    }
  }

  finalize(): AssembledToolCall[] {
    const out = [...this.byIndex.values()].filter(c => c.name);
    this.byIndex.clear();
    return out;
  }
}

export function tryParseJson<T = unknown>(text: string): T | undefined {
  try {
    return JSON.parse(text) as T;
  } catch {
    return undefined;
  }
}
