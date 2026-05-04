import { describe, expect, it } from 'vitest';
import { hasExplicitApiPath, resolveChatCompletionsUrl, resolveModelsUrl } from './url.js';

describe('hasExplicitApiPath', () => {
  it('returns true when /chat/completions is present', () => {
    expect(hasExplicitApiPath('https://x/v1/chat/completions')).toBe(true);
  });
  it('returns true when /responses is present', () => {
    expect(hasExplicitApiPath('https://x/v1/responses')).toBe(true);
  });
  it('returns false for a base url', () => {
    expect(hasExplicitApiPath('https://x/v1')).toBe(false);
  });
});

describe('resolveChatCompletionsUrl', () => {
  it('returns explicit chat/completions urls unchanged', () => {
    const u = 'https://api.openai.com/v1/chat/completions';
    expect(resolveChatCompletionsUrl(u)).toBe(u);
  });
  it('returns explicit responses urls unchanged', () => {
    const u = 'https://api.openai.com/v1/responses';
    expect(resolveChatCompletionsUrl(u)).toBe(u);
  });
  it('appends /chat/completions when url ends in /v1', () => {
    expect(resolveChatCompletionsUrl('https://api.openai.com/v1')).toBe(
      'https://api.openai.com/v1/chat/completions',
    );
  });
  it('handles cloudflare-style /ai/v1', () => {
    expect(
      resolveChatCompletionsUrl('https://api.cloudflare.com/client/v4/accounts/abc/ai/v1'),
    ).toBe('https://api.cloudflare.com/client/v4/accounts/abc/ai/v1/chat/completions');
  });
  it('strips a trailing slash before appending', () => {
    expect(resolveChatCompletionsUrl('https://api.openai.com/v1/')).toBe(
      'https://api.openai.com/v1/chat/completions',
    );
  });
  it('appends /v1/chat/completions when url has no version', () => {
    expect(resolveChatCompletionsUrl('https://api.openai.com')).toBe(
      'https://api.openai.com/v1/chat/completions',
    );
  });
  it('handles /v2 versioning', () => {
    expect(resolveChatCompletionsUrl('https://x/v2')).toBe('https://x/v2/chat/completions');
  });
});

describe('resolveModelsUrl', () => {
  it('appends /models to a versioned base', () => {
    expect(resolveModelsUrl('https://api.openai.com/v1')).toBe('https://api.openai.com/v1/models');
  });
  it('appends /v1/models to an unversioned base', () => {
    expect(resolveModelsUrl('https://x')).toBe('https://x/v1/models');
  });
  it('strips an explicit chat/completions path before appending', () => {
    expect(resolveModelsUrl('https://api.openai.com/v1/chat/completions')).toBe(
      'https://api.openai.com/v1/models',
    );
  });
});
