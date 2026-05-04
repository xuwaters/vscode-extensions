import * as vscode from 'vscode';
import type { ChatCompletionMessage, ChatCompletionRequest } from './accumulator.js';

export {
  ToolCallAccumulator,
  tryParseJson,
  type AssembledToolCall,
  type ChatCompletionMessage,
  type ChatCompletionRequest,
  type ChatCompletionTool,
  type ToolCallDelta,
} from './accumulator.js';

export function buildRequestBody(
  modelId: string,
  messages: readonly vscode.LanguageModelChatRequestMessage[],
  options: vscode.ProvideLanguageModelChatResponseOptions,
  toolCallingEnabled: boolean,
): ChatCompletionRequest {
  const body: ChatCompletionRequest = {
    model: modelId,
    messages: messages.flatMap(m => convertMessage(m)),
    stream: true,
  };

  const modelOptions = options.modelOptions ?? {};
  for (const [k, v] of Object.entries(modelOptions)) {
    if (v !== undefined) body[k] = v;
  }

  if (toolCallingEnabled && options.tools && options.tools.length > 0) {
    body.tools = options.tools.map(t => ({
      type: 'function',
      function: {
        name: t.name,
        description: t.description,
        parameters: t.inputSchema as object | undefined,
      },
    }));
    body.tool_choice = options.toolMode === vscode.LanguageModelChatToolMode.Required ? 'required' : 'auto';
  }

  return body;
}

function convertMessage(msg: vscode.LanguageModelChatRequestMessage): ChatCompletionMessage[] {
  const role = msg.role === vscode.LanguageModelChatMessageRole.Assistant ? 'assistant' : 'user';

  const textPieces: string[] = [];
  const toolCalls: NonNullable<ChatCompletionMessage['tool_calls']> = [];
  const toolResults: Array<{ callId: string; text: string }> = [];

  for (const part of msg.content) {
    if (part instanceof vscode.LanguageModelTextPart) {
      textPieces.push(part.value);
    } else if (part instanceof vscode.LanguageModelToolCallPart) {
      toolCalls.push({
        id: part.callId,
        type: 'function',
        function: {
          name: part.name,
          arguments: JSON.stringify(part.input ?? {}),
        },
      });
    } else if (part instanceof vscode.LanguageModelToolResultPart) {
      toolResults.push({ callId: part.callId, text: stringifyToolResult(part.content) });
    } else if (part instanceof vscode.LanguageModelDataPart) {
      if (part.mimeType.startsWith('text/')) {
        textPieces.push(new TextDecoder('utf-8').decode(part.data));
      }
    }
  }

  const out: ChatCompletionMessage[] = [];

  for (const tr of toolResults) {
    out.push({ role: 'tool', tool_call_id: tr.callId, content: tr.text });
  }

  const text = textPieces.join('');
  const hasText = text.length > 0;
  const hasToolCalls = toolCalls.length > 0;

  if (hasText || hasToolCalls || out.length === 0) {
    const result: ChatCompletionMessage = { role };
    if (hasText) result.content = text;
    if (hasToolCalls) result.tool_calls = toolCalls;
    if (msg.name) result.name = msg.name;
    if (result.content === undefined && !result.tool_calls) result.content = '';
    out.push(result);
  }

  return out;
}

function stringifyToolResult(parts: ReadonlyArray<unknown>): string {
  const out: string[] = [];
  for (const p of parts) {
    if (p instanceof vscode.LanguageModelTextPart) out.push(p.value);
    else if (p instanceof vscode.LanguageModelDataPart && p.mimeType.startsWith('text/')) {
      out.push(new TextDecoder('utf-8').decode(p.data));
    } else {
      try {
        out.push(JSON.stringify(p));
      } catch {
        // skip
      }
    }
  }
  return out.join('\n');
}
