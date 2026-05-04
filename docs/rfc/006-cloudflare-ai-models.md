# RFC 006: `cloudflare-ai-models` — User-Configured OpenAI-Compatible Model Provider

**Status**: Draft
**Date**: 2026-05-03
**Extension name**: `wx-vsce-cloudflare-ai-models`
**Primary motivating endpoint**: Cloudflare Workers AI Gateway (OpenAI-compatible)
**Primary motivating model**: `@cf/moonshotai/kimi-k2.6`

---

## 1. Motivation

GitHub Copilot Chat ships with a "Manage Models…" UI that exposes a fixed
set of BYOK (Bring Your Own Key) model providers: Anthropic, xAI, Google
Gemini, OpenRouter, OpenAI, Ollama, Azure. Each provider's "Add Model"
dialog asks for *only* the inputs that provider needs:

- Picking **OpenAI** asks for an **API key only**. There is no field for
  a custom base URL.
- Picking **Ollama** asks for a **URL only**. There is no field for an
  API key (Ollama assumes localhost-trusted).
- Picking **Azure** asks for an Azure-shaped endpoint, not a generic
  OpenAI-compatible one.

Neither shape fits the common case of "I have a deployment of an
OpenAI-compatible HTTP endpoint that requires both a custom URL **and**
an API key." Concrete examples:

- **Cloudflare Workers AI** — the AI Gateway exposes `@cf/...` models
  behind a URL like
  `https://api.cloudflare.com/client/v4/accounts/<ACCOUNT_ID>/ai/v1/chat/completions`
  and requires a Cloudflare API token in the `Authorization` header.
  The user's specific case: select `@cf/moonshotai/kimi-k2.6` against
  their own account, with their own token.
- **Self-hosted vLLM / TGI / LiteLLM gateways** — same shape: any
  URL, plus a bearer token.
- **Together / Groq / Fireworks / DeepInfra mirrors** — any "OpenAI
  shape" provider not on Copilot's stock list.

### 1.1 Why the obvious workarounds fall short

1. **Wait for Copilot to add a "CustomOAI" provider on stable.** It
   already exists in the source tree —
   [customOAIProvider.ts](../../temp/vscode/extensions/copilot/src/extension/byok/vscode-node/customOAIProvider.ts)
   — and is registered in
   [byokContribution.ts:63](../../temp/vscode/extensions/copilot/src/extension/byok/vscode-node/byokContribution.ts#L63),
   but the contribution in
   [package.json:1836-1838](../../temp/vscode/extensions/copilot/package.json#L1836-L1838)
   carries `"when": "productQualityType != 'stable'"` so it only
   surfaces in Insiders builds. The user is on stable. We have no
   control over when MS lifts that gate.
2. **Patch Copilot's `package.json`.** The Copilot extension is
   redistributed as a signed `.vsix`. Editing it in-place breaks
   signature verification, gets clobbered on every update, and
   couples our workflow to MS's release cadence.
3. **Use Continue / Cline / Roo Code.** All of these ship their own
   chat UI. The user explicitly wants the model to appear *inside the
   GitHub Copilot Chat picker* — i.e., reuse Copilot's chat
   experience, not stand up a parallel one.
4. **Run an Ollama-shaped shim in front of Cloudflare.** Possible but
   ugly: requires a long-lived local process, drops streaming
   semantics, and Copilot's Ollama provider hard-codes `http://localhost:11434`
   defaults and assumes no auth header.

### 1.2 The leverage point

VS Code exposes
[`lm.registerLanguageModelChatProvider`](https://code.visualstudio.com/api/references/vscode-api#lm.registerLanguageModelChatProvider)
as a public extension API. *Any* extension can register a
`LanguageModelChatProvider`, and the registered models become
selectable from `vscode.lm.selectChatModels()` and from the model
picker in Copilot Chat. This is the same API Copilot itself uses to
register its BYOK providers (see
[byokContribution.ts:65-67](../../temp/vscode/extensions/copilot/src/extension/byok/vscode-node/byokContribution.ts#L65-L67)).
There is nothing privileged about Copilot's registrations — a
side-loaded extension can publish a vendor with whatever
`configuration` schema we like (URL **and** API key, plus per-model
overrides), and Copilot Chat will treat the resulting models as
first-class entries in its picker.

This RFC proposes shipping that extension.

## 2. Goals and Non-Goals

### Goals

1. **A new model vendor in the Copilot Chat picker** named "OpenAI
   Compatible (Custom)", contributed by our extension via
   `contributes.languageModelChatProviders`.
2. **Both URL and API key** as required configuration inputs at the
   *vendor* (group) level, plus a per-model URL override.
3. **First-class support for Cloudflare Workers AI**: a one-click
   "Cloudflare Workers AI" preset that fills the URL template
   `https://api.cloudflare.com/client/v4/accounts/{ACCOUNT_ID}/ai/v1`
   and prompts only for the account ID + API token.
4. **Streaming chat completions** over SSE, with tool-calling support
   for models that advertise it (so Copilot's agent mode works).
5. **Token counting** good enough for Copilot's pre-flight context
   budgeting — a tiktoken-style estimator is sufficient; exact-match
   tokenization is non-goal.
6. **Zero dependency on the Copilot extension's internals.** We use
   only the public `vscode` API surface. Copilot can ship breaking
   changes to its BYOK code without breaking us.
7. **Side-loadable from this monorepo** — packaged as a `.vsix` like
   the other extensions in [extensions/](../../extensions/), no
   marketplace publication required.

### Non-Goals

- Reimplementing Copilot's full BYOK UI (the per-provider settings
  pages with model discovery, capability checkboxes, etc.). Our
  configuration lives in `settings.json` like the other extensions in
  this repo; the chat picker shows the resulting models.
- Inline completions (ghost text). Copilot's inline completions
  pipeline does not use `lm.registerLanguageModelChatProvider`; it
  uses its own proxy. This RFC is *chat only*.
- Embeddings, rerank, or other non-chat endpoints.
- A model marketplace / discovery UI. The user provides model IDs
  directly (e.g. `@cf/moonshotai/kimi-k2.6`).
- Provider-specific quirks beyond the OpenAI Chat Completions
  shape (Anthropic message format, Gemini's `generateContent`, etc.).
  Those would each be a separate provider; this RFC scopes to the
  OpenAI-compatible REST shape.
- Bypassing Copilot entitlement. The user must still have a working
  Copilot Chat installation — we contribute *to* it, not around it.

## 3. Design

### 3.1 Anatomy

```
extensions/cloudflare-ai-models/
  package.json              # name, contributes.languageModelChatProviders
  src/
    extension.ts            # activate(): register provider
    provider.ts             # OAICompatChatProvider implements LanguageModelChatProvider
    config.ts               # read settings, validate, watch for changes
    presets.ts              # Cloudflare / Together / Groq / generic templates
    sse.ts                  # streaming SSE parser (no extra dep)
    tokens.ts               # cheap token estimator (chars/4 fallback, gpt-tokenizer if small)
  tsdown.config.mts
  tsconfig.json
```

This mirrors the other extensions in [extensions/](../../extensions/)
(see e.g. [extensions/git-compare/](../../extensions/git-compare/)).
Bundling: `tsdown` → single `dist/extension.js`, same toolchain the
sibling extensions already use.

### 3.2 Manifest contribution

```jsonc
"contributes": {
  "languageModelChatProviders": [
    {
      "vendor": "wx-openai-compat",
      "displayName": "OpenAI Compatible (Custom)",
      "configuration": {
        "type": "object",
        "properties": {
          "url":    { "type": "string", "title": "Base URL",
                      "description": "OpenAI-compatible base URL, e.g. https://api.cloudflare.com/client/v4/accounts/<ACCOUNT_ID>/ai/v1" },
          "apiKey": { "type": "string", "secret": true, "title": "API Key" },
          "models": { "type": "array", "items": { "type": "object",
                      "properties": {
                        "id":             { "type": "string" },
                        "name":           { "type": "string" },
                        "url":            { "type": "string" },
                        "maxInputTokens": { "type": "number" },
                        "maxOutputTokens":{ "type": "number" },
                        "toolCalling":    { "type": "boolean" },
                        "vision":         { "type": "boolean" },
                        "requestHeaders": { "type": "object" }
                      },
                      "required": ["id"] } }
        },
        "required": ["url", "apiKey"]
      }
    }
  ]
}
```

The `secret: true` flag tells VS Code to store the value via
`SecretStorage` rather than `settings.json`. The `models` array lets a
user enumerate model IDs the endpoint serves; if it is empty we attempt
discovery by GETting `{url}/models` (the de-facto convention) and
filtering to entries with `id`.

The vendor will appear in Copilot Chat's "Manage Models…" picker
once VS Code surfaces it through the picker — the same mechanism that
exposes Copilot's own BYOK vendors.

### 3.3 Provider implementation

```ts
class OAICompatChatProvider implements vscode.LanguageModelChatProvider {
  async provideLanguageModelChatInformation(opts, token): Promise<LMChatInfo[]> {
    const { url, apiKey, models } = opts.configuration as Config;
    if (models?.length) return models.map(m => toLMChatInfo(m));
    return await discoverModels(url, apiKey, token);   // GET {url}/models
  }

  async provideLanguageModelChatResponse(model, messages, options, progress, token) {
    const body = toOpenAIChatBody(model, messages, options);  // tools, stream:true
    const res  = await fetch(joinUrl(model.url, '/chat/completions'), {
      method: 'POST',
      headers: { 'Authorization': `Bearer ${apiKey}`, 'Content-Type': 'application/json',
                 ...(model.requestHeaders ?? {}) },
      body: JSON.stringify(body), signal: abortFrom(token),
    });
    for await (const evt of parseSSE(res.body)) {
      const delta = evt.choices?.[0]?.delta;
      if (delta?.content) progress.report(new vscode.LanguageModelTextPart(delta.content));
      if (delta?.tool_calls) progress.report(toolCallPart(delta.tool_calls));
    }
  }

  async provideTokenCount(model, text, token): Promise<number> {
    return estimateTokens(typeof text === 'string' ? text : flatten(text));
  }
}
```

`abortFrom(token)` wires `CancellationToken` to an `AbortController`
so cancelling the chat aborts the in-flight HTTP request. This is the
single most user-visible failure mode if we get it wrong.

### 3.4 URL resolution

Reusing Copilot's logic from
[customOAIProvider.ts:19-45](../../temp/vscode/extensions/copilot/src/extension/byok/vscode-node/customOAIProvider.ts#L19-L45):

- If the URL already contains `/chat/completions` or `/responses`,
  treat it as fully-resolved.
- If it ends in `/v\d+`, append `/chat/completions`.
- Otherwise, append `/v1/chat/completions`.

For the Cloudflare preset specifically, the canonical base is
`.../ai/v1` — which falls through to the `/v\d+` branch, so we send to
`.../ai/v1/chat/completions`. Verified against Cloudflare's
documented OpenAI compatibility layer.

### 3.5 Cloudflare preset

A command `wx-openai-compat.addCloudflarePreset` opens a Quick Pick
that asks for:

1. Cloudflare account ID
2. Cloudflare API token (stored via `SecretStorage`)
3. (Optional) one or more model IDs, defaulting to a suggested list
   that includes `@cf/moonshotai/kimi-k2.6`

It writes the resulting block into the user's settings via
`migrateLanguageModelsProviderGroup` (the same command Copilot's BYOK
code uses to populate vendor configurations). This avoids hand-editing
JSON for the common case.

### 3.6 Token counting

Copilot uses token counts to budget context. Exact tokenization for
arbitrary models is impossible without per-model BPE files, so we ship
a cheap estimator: `Math.ceil(chars / 3.5)` for plain text, with
allowances for tool-call JSON. Empirically within ±15% of `o200k_base`
for English+code, which is well inside Copilot's context margin.

If the user really wants tighter accuracy, a follow-up can lazy-load
`gpt-tokenizer` (~1 MB) on first use. Out of scope for v0.1.

### 3.7 Tool calling

Models that advertise `toolCalling: true` get OpenAI-shaped `tools`
in the request body and we translate `tool_calls` deltas in the
streamed response into `LanguageModelToolCallPart`. Models without
the capability get plain text only — Copilot's agent mode will fall
back gracefully if it sees a non-tool-capable model.

The `kimi-k2.6` motivating model supports tools, so this matters for
day-1 usefulness.

### 3.8 Configuration sketch

```jsonc
// settings.json
"github.copilot.advanced.languageModelChatProviders": {
  "wx-openai-compat": {
    "url": "https://api.cloudflare.com/client/v4/accounts/abcd1234/ai/v1",
    // apiKey lives in SecretStorage, set via the preset command
    "models": [
      { "id": "@cf/moonshotai/kimi-k2.6", "name": "Kimi K2.6 (Cloudflare)",
        "maxInputTokens": 131072, "maxOutputTokens": 8192,
        "toolCalling": true,  "vision": false }
    ]
  }
}
```

The exact settings key is owned by VS Code's `lm` infrastructure once
the vendor is registered; the snippet shows shape, not the literal key
path. The point is: URL + API key + model list, all in one place.

## 4. Out-of-Scope Alternatives Considered

| Alternative | Why rejected |
|---|---|
| Fork the Copilot extension and flip the `when` clause | Re-signing + perpetual rebases; MS could change the BYOK schema underneath us |
| Patch the installed `.vsix` post-install | Same as above, plus breaks on every Copilot auto-update |
| Build a local Ollama-shaped shim in front of Cloudflare | Long-lived process; Copilot's Ollama provider drops auth headers; no streaming-tool-call coverage |
| Use `continue.dev` / Cline | User wants the model in the *Copilot Chat* picker, not a parallel UI |
| Wait for MS to flip the gate | No timeline; user wants it now; once it lands we can deprecate gracefully |

## 5. Risks and Open Questions

1. **Copilot Chat picker discovery.** The `lm.registerLanguageModelChatProvider`
   API is public, and Copilot's own BYOK providers go through it, so
   in principle our vendor should appear in the picker. **Verify
   on stable VS Code + stable Copilot before committing to this
   design.** If Copilot filters its picker to known vendors, we fall
   back to surfacing models only via `vscode.lm.selectChatModels()` —
   still useful for any other extension that uses the LM API, but
   loses the in-Copilot-UI integration.
2. **Tool-call schema drift.** OpenAI's `tools` schema has shifted
   over the years (functions → tools, parallel calls, JSON mode
   variations). Cloudflare's gateway tracks current OpenAI; other
   gateways may lag. Document tested gateways and pin schema to the
   2024-08 cut for v0.1.
3. **Streaming framing variance.** Some gateways send extra
   keep-alive comments or emit `data: [DONE]` early. SSE parser must
   tolerate both. Add fixture tests for Cloudflare, OpenAI, vLLM,
   LiteLLM responses.
4. **Settings schema collision with future Copilot CustomOAI on
   stable.** When MS lifts the `productQualityType != 'stable'` gate,
   users will have two ways to do the same thing. Mitigation: pick a
   distinct vendor id (`wx-openai-compat`, not `customoai`) so they
   coexist; document the migration path in the extension README.
5. **Secret leakage in logs.** Never log the Authorization header.
   Strip it from any error messages that include the request object.
6. **Cloudflare-specific request shape.** Cloudflare's docs claim
   strict OpenAI compatibility, but historically some `@cf/*` models
   ignored `temperature`/`top_p` or returned non-standard finish
   reasons. v0.1 will test against `@cf/moonshotai/kimi-k2.6`
   end-to-end and document any quirks discovered.

## 6. Phased Rollout

- **Phase 0 — Spike (1 day).** Stand up the bare extension; register
  a stub provider; confirm a hardcoded model appears in
  `vscode.lm.selectChatModels()` and in the Copilot Chat picker on
  stable. **This is the go/no-go gate for the whole RFC.**
- **Phase 1 — MVP.** Streaming chat completions, manual model list,
  URL + API key from settings/`SecretStorage`. End-to-end success
  with `@cf/moonshotai/kimi-k2.6` for plain Q&A.
- **Phase 2 — Tool calling.** Translate Copilot's tool definitions
  into OpenAI `tools`; parse streamed `tool_calls`; agent-mode loop
  works against Kimi.
- **Phase 3 — Presets & polish.** Cloudflare preset command; model
  discovery via `GET /models`; per-model overrides; README with
  Cloudflare setup walkthrough.
- **Phase 4 — Other gateway fixtures.** Add vLLM / LiteLLM / Together
  smoke fixtures so we catch shape regressions.

## 7. Testing Strategy

- **Unit**: URL resolver (port the table from
  [customOAIProvider.ts:19-45](../../temp/vscode/extensions/copilot/src/extension/byok/vscode-node/customOAIProvider.ts#L19-L45)),
  SSE parser, request body serialization, tool-call delta merge.
- **Integration**: replay recorded Cloudflare / OpenAI SSE streams
  against the provider; assert progress events and final tool-call
  shape. Fixtures live in `src/__fixtures__/`.
- **Manual**: load the `.vsix` in stable VS Code, configure a
  Cloudflare endpoint, confirm `@cf/moonshotai/kimi-k2.6` answers in
  Copilot Chat with streaming + tool calls.

Per repo convention
([feedback_test_location.md](../../../.claude/projects/-Users-will-Project-github-xuwaters-vscode-extensions/memory/feedback_test_location.md)),
all verification code lives as in-tree tests in the extension
package, not as `/tmp` scratch scripts.

## 8. Resolved Decisions

1. **Single extension with presets.** Ship one
   `wx-vsce-cloudflare-ai-models`, not per-gateway extensions. Presets
   handle gateway-specific ergonomics; users get one install point and
   one settings block.
2. **Curated model list, free-form fallback.** The Cloudflare preset
   Quick Pick shows a curated list (the Moonshot line including
   `@cf/moonshotai/kimi-k2.6`, `@cf/meta/llama-3.3-70b-instruct`,
   `@cf/qwen/...`, etc.) **plus** a "Custom model ID…" entry at the
   bottom that lets the user type any model name. Users can also add
   models post-hoc by editing the `models[]` array in settings or
   re-running the preset command. Curated entries carry sensible
   defaults for `maxInputTokens`, `maxOutputTokens`, `toolCalling`,
   and `vision`; free-form entries fall back to conservative defaults
   (128k in / 8k out, no tools, no vision) which the user can
   override in settings.
3. **Tool-calling fidelity for v0.2.** Start with the OpenAI
   Chat Completions `tools` schema (single-call-at-a-time, function
   shape) since that's what Cloudflare's gateway documents and what
   Kimi K2.6 actually supports. **Add parallel `tool_calls` array
   handling in v0.2** — it's a small delta on top of the streaming
   delta merger and Copilot's agent mode is increasingly likely to
   emit parallel tool plans. **Defer** strict JSON mode / structured
   outputs to v0.3 or until a concrete Copilot feature requests
   them; they're easy to bolt on once the body builder exists, and
   adding them speculatively risks shape drift across gateways that
   implement them inconsistently.
