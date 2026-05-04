export function hasExplicitApiPath(url: string): boolean {
  return url.includes('/responses') || url.includes('/chat/completions');
}

export function resolveChatCompletionsUrl(url: string): string {
  if (hasExplicitApiPath(url)) {
    return url;
  }

  let trimmed = url;
  while (trimmed.endsWith('/')) {
    trimmed = trimmed.slice(0, -1);
  }

  const versionPattern = /\/v\d+$/;
  if (versionPattern.test(trimmed)) {
    return `${trimmed}/chat/completions`;
  }

  return `${trimmed}/v1/chat/completions`;
}

export function resolveModelsUrl(url: string): string {
  let trimmed = url;
  while (trimmed.endsWith('/')) {
    trimmed = trimmed.slice(0, -1);
  }

  if (hasExplicitApiPath(trimmed)) {
    const cutAt = Math.min(
      trimmed.includes('/chat/completions') ? trimmed.indexOf('/chat/completions') : Number.MAX_SAFE_INTEGER,
      trimmed.includes('/responses') ? trimmed.indexOf('/responses') : Number.MAX_SAFE_INTEGER,
    );
    trimmed = trimmed.slice(0, cutAt);
  }

  const versionPattern = /\/v\d+$/;
  if (versionPattern.test(trimmed)) {
    return `${trimmed}/models`;
  }

  return `${trimmed}/v1/models`;
}
