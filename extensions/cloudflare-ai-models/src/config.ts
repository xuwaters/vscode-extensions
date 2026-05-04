import * as vscode from 'vscode';

export { effectiveModelUrl, mergeHeaders } from './headers.js';

export const CONFIG_SECTION = 'wxCloudflareAi';
export const SECRET_API_KEY = 'wxCloudflareAi.apiKey';

export interface ModelConfig {
  id: string;
  name?: string;
  family?: string;
  url?: string;
  maxInputTokens?: number;
  maxOutputTokens?: number;
  toolCalling?: boolean;
  vision?: boolean;
  requestHeaders?: Record<string, string>;
}

export interface ProviderConfig {
  url: string;
  models: ModelConfig[];
  requestHeaders: Record<string, string>;
}

export function readConfig(): ProviderConfig {
  const cfg = vscode.workspace.getConfiguration(CONFIG_SECTION);
  const url = (cfg.get<string>('url') ?? '').trim();
  const rawModels = cfg.get<unknown[]>('models') ?? [];
  const requestHeaders = cfg.get<Record<string, string>>('requestHeaders') ?? {};
  const models: ModelConfig[] = [];
  for (const entry of rawModels) {
    if (!entry || typeof entry !== 'object') continue;
    const e = entry as Record<string, unknown>;
    if (typeof e.id !== 'string' || !e.id) continue;
    models.push({
      id: e.id,
      name: typeof e.name === 'string' ? e.name : undefined,
      family: typeof e.family === 'string' ? e.family : undefined,
      url: typeof e.url === 'string' ? e.url : undefined,
      maxInputTokens: typeof e.maxInputTokens === 'number' ? e.maxInputTokens : undefined,
      maxOutputTokens: typeof e.maxOutputTokens === 'number' ? e.maxOutputTokens : undefined,
      toolCalling: typeof e.toolCalling === 'boolean' ? e.toolCalling : undefined,
      vision: typeof e.vision === 'boolean' ? e.vision : undefined,
      requestHeaders: isStringRecord(e.requestHeaders) ? e.requestHeaders : undefined,
    });
  }
  return { url, models, requestHeaders };
}

function isStringRecord(v: unknown): v is Record<string, string> {
  if (!v || typeof v !== 'object') return false;
  for (const val of Object.values(v as Record<string, unknown>)) {
    if (typeof val !== 'string') return false;
  }
  return true;
}

export async function getApiKey(secrets: vscode.SecretStorage): Promise<string | undefined> {
  return secrets.get(SECRET_API_KEY);
}

export async function setApiKey(secrets: vscode.SecretStorage, value: string): Promise<void> {
  await secrets.store(SECRET_API_KEY, value);
}

export async function clearApiKey(secrets: vscode.SecretStorage): Promise<void> {
  await secrets.delete(SECRET_API_KEY);
}

