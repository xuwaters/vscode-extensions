import { describe, expect, it } from 'vitest';
import { CURATED, catalogWithUserModels, normalizeModelId } from './catalog.js';

describe('normalizeModelId', () => {
  it('prefixes bare @cf ids', () => {
    expect(normalizeModelId('@cf/google/gemma-3-12b-it')).toBe('workers-ai/@cf/google/gemma-3-12b-it');
  });

  it('leaves already-prefixed and foreign ids alone', () => {
    expect(normalizeModelId(' workers-ai/@cf/qwen/qwen3-30b-a3b-fp8 ')).toBe('workers-ai/@cf/qwen/qwen3-30b-a3b-fp8');
    expect(normalizeModelId('llama-3.1-70b')).toBe('llama-3.1-70b');
  });
});

describe('catalogWithUserModels', () => {
  it('lists every curated preset when settings are empty', () => {
    const ids = catalogWithUserModels([]).map(m => m.id);
    expect(ids).toEqual(CURATED.map(m => normalizeModelId(m.id)));
  });

  it('does not duplicate a curated model the preset already wrote to settings', () => {
    const merged = catalogWithUserModels([{ id: 'workers-ai/@cf/zai-org/glm-5.2', name: 'GLM 5.2 (Cloudflare)' }]);
    expect(merged.filter(m => m.id === 'workers-ai/@cf/zai-org/glm-5.2')).toHaveLength(1);
    expect(merged).toHaveLength(CURATED.length);
  });

  it('keeps curated metadata when the user entry omits it', () => {
    const [glm] = catalogWithUserModels([
      { id: '@cf/zai-org/glm-5.2', name: undefined, family: undefined, toolCalling: undefined, url: 'https://alt/v1' },
    ]).filter(m => m.id === 'workers-ai/@cf/zai-org/glm-5.2');
    expect(glm.name).toBe('GLM 5.2 (Cloudflare)');
    expect(glm.toolCalling).toBe(true);
    expect(glm.url).toBe('https://alt/v1');
  });

  it('lets a user entry override curated metadata and appends unknown ids', () => {
    const merged = catalogWithUserModels([
      { id: 'workers-ai/@cf/qwen/qwen3-30b-a3b-fp8', maxInputTokens: 999 },
      { id: 'my-vllm/llama-3.1-70b', name: 'Local Llama' },
    ]);
    expect(merged.find(m => m.id === 'workers-ai/@cf/qwen/qwen3-30b-a3b-fp8')?.maxInputTokens).toBe(999);
    expect(merged.at(-1)).toMatchObject({ id: 'my-vllm/llama-3.1-70b', name: 'Local Llama' });
  });
});
