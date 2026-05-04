import * as vscode from 'vscode';
import {
  effectiveModelUrl,
  getApiKey,
  mergeHeaders,
  readConfig,
  type ModelConfig,
  type ProviderConfig,
} from './config.js';
import { buildRequestBody, ToolCallAccumulator, tryParseJson } from './openai.js';
import { isSSEDone, parseSSE } from './sse.js';
import { estimateMessageTokens, estimateTokens } from './tokens.js';
import { resolveChatCompletionsUrl } from './url.js';

export const VENDOR = 'wx-cloudflare-ai';

interface CloudflareModelInfo extends vscode.LanguageModelChatInformation {
  readonly _config: ModelConfig;
  readonly isUserSelectable?: boolean;
}

export class CloudflareAIChatProvider implements vscode.LanguageModelChatProvider<CloudflareModelInfo> {
  private readonly _onDidChange = new vscode.EventEmitter<void>();
  readonly onDidChangeLanguageModelChatInformation = this._onDidChange.event;

  constructor(private readonly secrets: vscode.SecretStorage) {}

  fireChange(): void {
    this._onDidChange.fire();
  }

  async provideLanguageModelChatInformation(
    options: vscode.PrepareLanguageModelChatModelOptions,
    _token: vscode.CancellationToken,
  ): Promise<CloudflareModelInfo[]> {
    const cfg = readConfig();
    if (!cfg.url) {
      if (!options.silent) {
        void vscode.window.showWarningMessage(
          'Cloudflare AI: set wxCloudflareAi.url and wxCloudflareAi.models in settings, or run "Add Cloudflare AI Gateway Preset".',
        );
      }
      return [];
    }
    if (cfg.models.length === 0) {
      if (!options.silent) {
        void vscode.window.showWarningMessage(
          'Cloudflare AI: configure at least one entry in wxCloudflareAi.models.',
        );
      }
      return [];
    }
    return cfg.models.map(m => toModelInfo(m));
  }

  async provideLanguageModelChatResponse(
    model: CloudflareModelInfo,
    messages: readonly vscode.LanguageModelChatRequestMessage[],
    options: vscode.ProvideLanguageModelChatResponseOptions,
    progress: vscode.Progress<vscode.LanguageModelResponsePart>,
    token: vscode.CancellationToken,
  ): Promise<void> {
    const cfg = readConfig();
    const apiKey = await getApiKey(this.secrets);
    if (!apiKey) {
      throw new Error(
        'Cloudflare AI: API key not set. Run the "Cloudflare AI: Set API Key" command.',
      );
    }

    const baseUrl = effectiveModelUrl(model._config, cfg.url);
    if (!baseUrl) {
      throw new Error('Cloudflare AI: no URL configured for this model.');
    }
    const endpoint = resolveChatCompletionsUrl(baseUrl);
    const headers: Record<string, string> = {
      ...mergeHeaders(cfg.requestHeaders, model._config.requestHeaders),
      Authorization: `Bearer ${apiKey}`,
      'Content-Type': 'application/json',
      Accept: 'text/event-stream',
    };
    const body = buildRequestBody(model._config.id, messages, options, model.capabilities.toolCalling === true);

    const controller = new AbortController();
    const cancelSub = token.onCancellationRequested(() => controller.abort());

    let res: Response;
    try {
      res = await fetch(endpoint, {
        method: 'POST',
        headers,
        body: JSON.stringify(body),
        signal: controller.signal,
      });
    } catch (err) {
      cancelSub.dispose();
      throw rewrapError(err, endpoint);
    }

    if (!res.ok || !res.body) {
      const text = await safeReadText(res);
      cancelSub.dispose();
      throw new Error(
        `Cloudflare AI: ${res.status} ${res.statusText} from ${endpoint}${text ? ` — ${truncate(text, 500)}` : ''}`,
      );
    }

    const accumulator = new ToolCallAccumulator();
    try {
      for await (const evt of parseSSE(res.body as unknown as AsyncIterable<Uint8Array>)) {
        if (token.isCancellationRequested) break;
        if (isSSEDone(evt)) break;
        const parsed = tryParseJson<ChatCompletionStreamChunk>(evt.data);
        if (!parsed) continue;
        const choice = parsed.choices?.[0];
        const delta = choice?.delta;
        if (!delta) continue;
        if (typeof delta.content === 'string' && delta.content.length > 0) {
          progress.report(new vscode.LanguageModelTextPart(delta.content));
        }
        if (delta.tool_calls) {
          accumulator.ingest(delta.tool_calls);
        }
        if (choice?.finish_reason) {
          break;
        }
      }
      for (const call of accumulator.finalize()) {
        const input = tryParseJson<object>(call.arguments) ?? {};
        progress.report(new vscode.LanguageModelToolCallPart(call.callId || cryptoRandomId(), call.name, input));
      }
    } finally {
      cancelSub.dispose();
    }
  }

  async provideTokenCount(
    _model: CloudflareModelInfo,
    text: string | vscode.LanguageModelChatRequestMessage,
    _token: vscode.CancellationToken,
  ): Promise<number> {
    if (typeof text === 'string') return estimateTokens(text);
    return estimateMessageTokens(roleToString(text.role), text.content);
  }
}

interface ChatCompletionStreamChunk {
  choices?: Array<{
    delta?: {
      content?: string;
      tool_calls?: Array<{
        index?: number;
        id?: string;
        type?: 'function';
        function?: { name?: string; arguments?: string };
      }>;
    };
    finish_reason?: string | null;
  }>;
}

function toModelInfo(m: ModelConfig): CloudflareModelInfo {
  return {
    id: m.id,
    name: m.name ?? m.id,
    family: m.family ?? deriveFamily(m.id),
    version: '1',
    maxInputTokens: m.maxInputTokens ?? 128_000,
    maxOutputTokens: m.maxOutputTokens ?? 8_192,
    capabilities: {
      imageInput: m.vision === true,
      toolCalling: m.toolCalling === true,
    },
    // Show the model in the Copilot Chat model picker by default. Field is on
    // the proposed `chatProvider` API but VS Code reads it off the returned
    // object regardless, so it works on stable too.
    isUserSelectable: true,
    _config: m,
  };
}

function deriveFamily(id: string): string {
  const slash = id.lastIndexOf('/');
  return slash === -1 ? id : id.slice(slash + 1);
}

function roleToString(role: vscode.LanguageModelChatMessageRole): string {
  return role === vscode.LanguageModelChatMessageRole.Assistant ? 'assistant' : 'user';
}

async function safeReadText(res: Response): Promise<string | undefined> {
  try {
    return await res.text();
  } catch {
    return undefined;
  }
}

function truncate(s: string, n: number): string {
  return s.length > n ? `${s.slice(0, n)}…` : s;
}

function rewrapError(err: unknown, endpoint: string): Error {
  if (err instanceof Error && err.name === 'AbortError') {
    return new vscode.CancellationError();
  }
  const msg = err instanceof Error ? err.message : String(err);
  return new Error(`Cloudflare AI: request to ${endpoint} failed — ${msg}`);
}

function cryptoRandomId(): string {
  return `call_${Math.random().toString(36).slice(2, 10)}`;
}

export type { CloudflareModelInfo, ProviderConfig };
