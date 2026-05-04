import * as vscode from 'vscode';
import { CONFIG_SECTION, setApiKey, type ModelConfig } from './config.js';

const CLOUDFLARE_URL_TEMPLATE = 'https://api.cloudflare.com/client/v4/accounts/{ACCOUNT_ID}/ai/v1';

interface CuratedModel {
  id: string;
  name: string;
  family: string;
  maxInputTokens: number;
  maxOutputTokens: number;
  toolCalling: boolean;
  vision: boolean;
}

const CURATED: CuratedModel[] = [
  {
    id: '@cf/moonshotai/kimi-k2.6',
    name: 'Kimi K2.6 (Cloudflare)',
    family: 'kimi-k2',
    maxInputTokens: 262_144,
    maxOutputTokens: 16_384,
    toolCalling: true,
    vision: false,
  },
  {
    id: '@cf/qwen/qwen3-30b-a3b-fp8',
    name: 'Qwen3 30B (Cloudflare)',
    family: 'qwen3',
    maxInputTokens: 32_768,
    maxOutputTokens: 8_192,
    toolCalling: true,
    vision: false,
  },
];

const FREEFORM_DEFAULTS = {
  maxInputTokens: 128_000,
  maxOutputTokens: 8_192,
  toolCalling: false,
  vision: false,
};

export async function runCloudflarePreset(secrets: vscode.SecretStorage): Promise<void> {
  const accountId = await vscode.window.showInputBox({
    title: 'Cloudflare Account ID',
    prompt: 'The 32-character ID from your Cloudflare dashboard URL.',
    ignoreFocusOut: true,
    validateInput: v => (v.trim() ? undefined : 'Account ID is required'),
  });
  if (!accountId) return;

  const apiToken = await vscode.window.showInputBox({
    title: 'Cloudflare API Token',
    prompt: 'API token with Workers AI Read permission. Stored in VS Code SecretStorage.',
    password: true,
    ignoreFocusOut: true,
    validateInput: v => (v.trim() ? undefined : 'API token is required'),
  });
  if (!apiToken) return;

  const picked = await pickModels();
  if (!picked || picked.length === 0) return;

  const baseUrl = CLOUDFLARE_URL_TEMPLATE.replace('{ACCOUNT_ID}', accountId.trim());
  const cfg = vscode.workspace.getConfiguration(CONFIG_SECTION);

  const existing = (cfg.get<unknown[]>('models') ?? []) as ModelConfig[];
  const merged = mergeModels(existing, picked);

  await cfg.update('url', baseUrl, vscode.ConfigurationTarget.Global);
  await cfg.update('models', merged, vscode.ConfigurationTarget.Global);
  await setApiKey(secrets, apiToken.trim());

  void vscode.window.showInformationMessage(
    `OpenAI Compatible: Cloudflare preset configured (${picked.length} model${picked.length === 1 ? '' : 's'}).`,
  );
}

async function pickModels(): Promise<ModelConfig[] | undefined> {
  const items: Array<vscode.QuickPickItem & { _model?: CuratedModel; _custom?: true }> = CURATED.map(m => ({
    label: m.id,
    description: m.name,
    picked: m.id === '@cf/moonshotai/kimi-k2.6',
    _model: m,
  }));
  items.push({ label: '$(add) Custom model ID…', description: 'Enter any model id served by your endpoint', _custom: true });

  const picks = await vscode.window.showQuickPick(items, {
    canPickMany: true,
    placeHolder: 'Select Cloudflare models to register (Space toggles, Enter confirms)',
    ignoreFocusOut: true,
  });
  if (!picks || picks.length === 0) return undefined;

  const out: ModelConfig[] = [];
  for (const p of picks) {
    if (p._model) {
      out.push(curatedToConfig(p._model));
    } else if (p._custom) {
      const id = await vscode.window.showInputBox({
        title: 'Custom Cloudflare model id',
        prompt: 'e.g. @cf/google/gemma-3-12b-it',
        ignoreFocusOut: true,
        validateInput: v => (v.trim() ? undefined : 'Model id is required'),
      });
      if (id?.trim()) {
        out.push({
          id: id.trim(),
          name: id.trim(),
          ...FREEFORM_DEFAULTS,
        });
      }
    }
  }
  return out;
}

function curatedToConfig(m: CuratedModel): ModelConfig {
  return {
    id: m.id,
    name: m.name,
    family: m.family,
    maxInputTokens: m.maxInputTokens,
    maxOutputTokens: m.maxOutputTokens,
    toolCalling: m.toolCalling,
    vision: m.vision,
  };
}

function mergeModels(existing: ModelConfig[], incoming: ModelConfig[]): ModelConfig[] {
  const byId = new Map<string, ModelConfig>();
  for (const m of existing) {
    if (m && typeof m === 'object' && typeof m.id === 'string') byId.set(m.id, m);
  }
  for (const m of incoming) {
    byId.set(m.id, m);
  }
  return [...byId.values()];
}
