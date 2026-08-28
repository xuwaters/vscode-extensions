# Cloudflare AI Models

Registers Cloudflare Workers AI and AI Gateway models as VS Code chat language
models, so GitHub Copilot Chat — and any other extension that asks VS Code for a
language model — can call them. Chat completions stream over the
OpenAI-compatible API with tool calling; the API token lives in VS Code's
SecretStorage, never in settings.

## Features

- **A chat model provider**, registered under the vendor `wx-cloudflare-ai` and
  shown as **Cloudflare AI** in VS Code's model management UI. Every model it
  advertises is marked user-selectable, so it turns up in the Copilot Chat model
  picker alongside the built-in models. VS Code remembers which ones you
  enabled — the provider always lists the full catalogue, it does not force
  models on.

- **A guided setup preset** (**Cloudflare AI: Add Cloudflare AI Gateway
  Preset**) that asks for your endpoint URL, then your API token, then shows a
  multi-select list of the curated models. It writes `wxCloudflareAi.url` and
  `wxCloudflareAi.models` to your user settings and stores the token in
  SecretStorage. Existing entries in `wxCloudflareAi.models` are kept; an
  incoming model with the same id replaces the old entry.

- **Curated Cloudflare models**, always offered once `wxCloudflareAi.url` is
  set, with their context limits and capabilities filled in:

  | Model id | Name | Input | Output | Tools | Vision |
  | --- | --- | --- | --- | --- | --- |
  | `workers-ai/@cf/deepseek-ai/deepseek-v4-pro-0813` | DeepSeek V4 Pro | 1048576 | 16384 | yes | no |
  | `workers-ai/@cf/deepseek-ai/deepseek-v4-flash-0731` | DeepSeek V4 Flash | 1048576 | 16384 | yes | no |
  | `workers-ai/@cf/zai-org/glm-5.3-flash` | GLM 5.3 Flash | 1048576 | 16384 | yes | yes |
  | `workers-ai/@cf/zai-org/glm-5.2` | GLM 5.2 | 262144 | 16384 | yes | no |
  | `workers-ai/@cf/moonshotai/kimi-k2.6` | Kimi K2.6 | 262144 | 16384 | yes | yes |
  | `workers-ai/@cf/moonshotai/kimi-k2.7-code` | Kimi K2.7 Code | 262144 | 16384 | yes | yes |
  | `workers-ai/@cf/qwen/qwen3-30b-a3b-fp8` | Qwen3 30B | 32768 | 8192 | yes | no |

  DeepSeek V4 Pro, GLM 5.3 Flash, GLM 5.2, Kimi K2.6 and Kimi K2.7 Code come
  pre-ticked in the preset's picker. Model ids are normalised: a bare `@cf/…` id
  is rewritten to `workers-ai/@cf/…`, so you can paste either form.

- **Any other model id**, via the "Custom model ID…" entry in the preset picker
  or by adding it to `wxCloudflareAi.models`. Repeating a curated id in settings
  overrides just the fields you specify — name, family, token limits,
  capabilities, per-model URL or per-model headers — and leaves the rest of the
  curated metadata intact.

- **Streaming responses.** Requests are POSTed with `stream: true` and
  `Accept: text/event-stream`; the SSE stream is parsed incrementally and text
  deltas are reported to VS Code as they arrive. The stream stops on the first
  `finish_reason` or on `data: [DONE]`. Cancelling a request in the chat view
  aborts the in-flight HTTP request.

- **Tool calling**, for models whose `toolCalling` is true and only when the
  caller supplied tools. The tool list is sent as OpenAI `tools`, with
  `tool_choice` set to `required` when VS Code asks for it and `auto`
  otherwise. Streamed `tool_calls` deltas are reassembled by index — name and
  argument fragments concatenated — and emitted once the stream ends. Tool
  results are sent back as `role: "tool"` messages keyed by call id.

- **Per-model and per-request escape hatches.** Any extra options the calling
  extension passes are merged into the JSON request body verbatim, so
  `temperature`, `top_p` and friends pass straight through. Extra HTTP headers
  can be set globally and per model. `Authorization`, `Content-Type`,
  `Content-Length`, `Host` and `Connection` are managed by the extension and are
  silently dropped from your header overrides.

- **Works against non-Cloudflare endpoints too.** Nothing in the request path is
  Cloudflare-specific beyond the default URLs and model ids, so a self-hosted
  vLLM or LiteLLM server, or any other OpenAI-compatible endpoint, works by
  pointing `wxCloudflareAi.url` at it and listing its model ids in
  `wxCloudflareAi.models`.

## Requirements

- An OpenAI-compatible endpoint URL. For Cloudflare that is either an AI Gateway
  compatibility endpoint,
  `https://gateway.ai.cloudflare.com/v1/<ACCOUNT_ID>/<GATEWAY_ID>/compat`, or
  Workers AI directly,
  `https://api.cloudflare.com/client/v4/accounts/<ACCOUNT_ID>/ai/v1`. Your
  Cloudflare account id is part of that URL — there is no separate account-id
  setting.

- A Cloudflare API token. The setup preset asks for a token with the **Workers
  AI Read** permission, which is what the Workers AI and AI Gateway chat
  completion endpoints require. It is sent as `Authorization: Bearer <token>`
  and is stored in VS Code's SecretStorage under `wxCloudflareAi.apiKey` — not
  in `settings.json`, and there is no environment-variable fallback.

The only endpoint this extension calls is the chat completions path derived
from your URL (see below). It never lists models over the network, never
touches the Cloudflare account or gateway management APIs, and makes no request
at all until a chat model is actually invoked.

## Getting started

1. Run **Cloudflare AI: Add Cloudflare AI Gateway Preset** from the Command
   Palette. (It is also wired up as the manage action next to **Cloudflare AI**
   in VS Code's language model management UI.)
2. Paste your endpoint URL. It must be a valid `http://` or `https://` URL;
   trailing slashes are stripped.
3. Paste your Cloudflare API token. The input is masked.
4. Tick the models you want — Space toggles, Enter confirms — and optionally
   add a custom model id. Custom ids default to 128000 input tokens, 8192
   output tokens, and no tool calling or vision.
5. Open Copilot Chat, switch the model picker to one of the Cloudflare models,
   and send a message.

If `wxCloudflareAi.url` is empty when something asks for a model, the extension
warns you and advertises nothing. If the URL is set but no API token has been
stored, the request fails with a message pointing at **Cloudflare AI: Set API
Key**. Changing any `wxCloudflareAi.*` setting re-publishes the model list
immediately; **Cloudflare AI: Refresh Models** does the same on demand.

## How the request URL is built

`wxCloudflareAi.url` (or a model's own `url`) is turned into a chat completions
endpoint by these rules, in order:

- If it already contains `/chat/completions` or `/responses`, it is used
  exactly as given.
- Otherwise, trailing slashes are stripped. If what remains ends in a version
  segment such as `/v1` or `/v2`, `/chat/completions` is appended.
- Otherwise `/v1/chat/completions` is appended.

So `https://your-host/v1` becomes `https://your-host/v1/chat/completions`, and
`https://your-host` becomes `https://your-host/v1/chat/completions` as well. If
your endpoint does not follow that shape, give the full URL including
`/chat/completions` and it will be used untouched.

## Settings

| Setting | Default | Description |
| --- | --- | --- |
| `wxCloudflareAi.url` | `""` | Endpoint URL for every model that does not override it. Empty means the provider advertises no models. |
| `wxCloudflareAi.requestHeaders` | `{}` | Extra HTTP headers sent with every request. Per-model `requestHeaders` win over these; reserved headers are ignored. |
| `wxCloudflareAi.models` | `[]` | Extra models served by the endpoint, or overrides for a curated model with the same id. Entries without a string `id` are ignored. |

Each entry in `wxCloudflareAi.models` is an object; only `id` is required.

| Field | Default | Description |
| --- | --- | --- |
| `id` | — | Model identifier sent in the request body, e.g. `@cf/google/gemma-3-12b-it`. |
| `name` | the id | Display name in the model picker. |
| `family` | last path segment of the id | Opaque family identifier. |
| `url` | `wxCloudflareAi.url` | Per-model base URL override. |
| `maxInputTokens` | `128000` | Advertised context window. |
| `maxOutputTokens` | `8192` | Advertised maximum completion length. |
| `toolCalling` | `false` | Whether tools may be sent to this model. |
| `vision` | `false` | Advertised image-input capability. |
| `requestHeaders` | — | Headers merged on top of `wxCloudflareAi.requestHeaders`. |

## Commands

- **Cloudflare AI: Add Cloudflare AI Gateway Preset**
  (`wx-cloudflare-ai.addCloudflarePreset`) — run the guided setup described
  above.
- **Cloudflare AI: Set API Key** (`wx-cloudflare-ai.setApiKey`) — store or
  replace the bearer token in SecretStorage without touching the URL or model
  list.
- **Cloudflare AI: Clear API Key** (`wx-cloudflare-ai.clearApiKey`) — delete the
  stored token.
- **Cloudflare AI: Refresh Models** (`wx-cloudflare-ai.refreshModels`) —
  re-publish the model list from current settings. This re-reads settings; it
  does not query the endpoint for available models.

## Notes and limits

- **Token counts are estimates.** VS Code's token-counting call is answered with
  a character-based heuristic (roughly 3.5 characters per token), not the
  model's real tokenizer, so budget calculations are approximate.

- **Requests carry text.** Message text and `text/*` data parts are forwarded;
  data parts with other MIME types are dropped, so the `vision` flag currently
  affects only what the provider advertises to VS Code.

- **Roles are collapsed to `user` and `assistant`.** Anything VS Code does not
  mark as an assistant message is sent with `role: "user"`; tool results are
  sent separately as `role: "tool"`.

- **Errors are surfaced verbatim.** A non-2xx response raises an error naming
  the status, the endpoint and up to 500 characters of the response body, which
  is usually enough to tell a bad token from a wrong model id.
