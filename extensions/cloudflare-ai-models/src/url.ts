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

export const GATEWAY_URL_FORM = 'https://gateway.ai.cloudflare.com/v1/<ACCOUNT_ID>/<GATEWAY_ID>/compat';
export const WORKERS_AI_URL_FORM = 'https://api.cloudflare.com/client/v4/accounts/<ACCOUNT_ID>/ai/v1';

/**
 * Why `url` is not a Cloudflare AI endpoint, or `undefined` when it is one.
 *
 * The API token rides on every request as `Authorization: Bearer`, so the
 * only hosts it may go to are Cloudflare's own: an AI Gateway endpoint or
 * Workers AI direct, over https, with no user info or port to smuggle a
 * different destination in.
 */
export function cloudflareUrlProblem(url: string): string | undefined {
  let parsed: URL;
  try {
    parsed = new URL(url.trim());
  } catch {
    return 'not a valid URL';
  }
  if (parsed.protocol !== 'https:') return 'https:// is required';
  if (parsed.username || parsed.password || parsed.port) {
    return 'a user name, password or port is not allowed';
  }
  const segments = parsed.pathname.split('/').filter(Boolean);
  if (parsed.hostname === 'gateway.ai.cloudflare.com') {
    return segments[0] === 'v1' && segments.length >= 3
      ? undefined
      : `an AI Gateway URL has the form ${GATEWAY_URL_FORM}`;
  }
  if (parsed.hostname === 'api.cloudflare.com') {
    const [client, v4, accounts, account, ai] = segments;
    return client === 'client' && v4 === 'v4' && accounts === 'accounts' && account && ai === 'ai'
      ? undefined
      : `a Workers AI URL has the form ${WORKERS_AI_URL_FORM}`;
  }
  return 'only gateway.ai.cloudflare.com and api.cloudflare.com are accepted';
}
