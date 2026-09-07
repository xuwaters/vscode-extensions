import type { ModelConfig } from './config.js';

export interface CuratedModel {
  id: string;
  name: string;
  family: string;
  maxInputTokens: number;
  maxOutputTokens: number;
  toolCalling: boolean;
  vision: boolean;
  defaultPicked?: boolean;
}

export const CURATED: CuratedModel[] = [
  {
    id: 'workers-ai/@cf/deepseek-ai/deepseek-v4-pro-0813',
    name: 'DeepSeek V4 Pro (Cloudflare)',
    family: 'deepseek-v4',
    maxInputTokens: 1_048_576,
    maxOutputTokens: 16_384,
    toolCalling: true,
    vision: false,
    defaultPicked: true,
  },
  {
    id: 'workers-ai/@cf/deepseek-ai/deepseek-v4-flash-0731',
    name: 'DeepSeek V4 Flash (Cloudflare)',
    family: 'deepseek-v4',
    maxInputTokens: 1_048_576,
    maxOutputTokens: 16_384,
    toolCalling: true,
    vision: false,
  },
  {
    id: 'workers-ai/@cf/zai-org/glm-5.3-flash',
    name: 'GLM 5.3 Flash (Cloudflare)',
    family: 'glm-5.3',
    maxInputTokens: 1_048_576,
    maxOutputTokens: 16_384,
    toolCalling: true,
    vision: true,
    defaultPicked: true,
  },
  {
    id: 'workers-ai/@cf/zai-org/glm-5.3',
    name: 'GLM 5.3 (Cloudflare)',
    family: 'glm-5.3',
    maxInputTokens: 1_048_576,
    maxOutputTokens: 16_384,
    toolCalling: true,
    vision: false,
    defaultPicked: true,
  },
  {
    id: 'workers-ai/@cf/moonshotai/kimi-k2.6',
    name: 'Kimi K2.6 (Cloudflare)',
    family: 'kimi-k2',
    maxInputTokens: 262_144,
    maxOutputTokens: 16_384,
    toolCalling: true,
    vision: true,
    defaultPicked: true,
  },
  {
    id: 'workers-ai/@cf/moonshotai/kimi-k2.7-code',
    name: 'Kimi K2.7 Code (Cloudflare)',
    family: 'kimi-k2',
    maxInputTokens: 262_144,
    maxOutputTokens: 16_384,
    toolCalling: true,
    vision: true,
    defaultPicked: true,
  },
  {
    id: 'workers-ai/@cf/qwen/qwen3-30b-a3b-fp8',
    name: 'Qwen3 30B (Cloudflare)',
    family: 'qwen3',
    maxInputTokens: 32_768,
    maxOutputTokens: 8_192,
    toolCalling: true,
    vision: false,
  },
];

export function normalizeModelId(id: string): string {
  const trimmed = id.trim();
  if (trimmed.startsWith('workers-ai/')) {
    return trimmed;
  }
  if (trimmed.startsWith('@cf/')) {
    return `workers-ai/${trimmed}`;
  }
  return trimmed;
}

export function curatedToConfig(m: CuratedModel): ModelConfig {
  return {
    id: normalizeModelId(m.id),
    name: m.name,
    family: m.family,
    maxInputTokens: m.maxInputTokens,
    maxOutputTokens: m.maxOutputTokens,
    toolCalling: m.toolCalling,
    vision: m.vision,
  };
}

/**
 * The full model list the provider advertises: every curated preset, plus any
 * extra ids the user put in `wxCloudflareAi.models`. A settings entry wins over
 * the curated one with the same id, so per-model `url`/`requestHeaders`/limit
 * overrides keep working. VS Code remembers which of these the user enabled,
 * so listing all of them just populates the picker — it does not force them on.
 */
export function catalogWithUserModels(userModels: ModelConfig[]): ModelConfig[] {
  const byId = new Map<string, ModelConfig>();
  for (const m of CURATED) {
    byId.set(normalizeModelId(m.id), curatedToConfig(m));
  }
  for (const m of userModels) {
    const id = normalizeModelId(m.id);
    byId.set(id, { ...byId.get(id), ...definedOnly(m), id });
  }
  return [...byId.values()];
}

// readConfig() materialises absent fields as `undefined`; spreading those over a
// curated entry would wipe its metadata.
function definedOnly(m: ModelConfig): Partial<ModelConfig> {
  const out: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(m)) {
    if (v !== undefined) out[k] = v;
  }
  return out as Partial<ModelConfig>;
}
