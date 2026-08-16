# Base64 Tools

Encode and decode Base64 directly in the editor. Select some text, run a
command, and the selection is replaced in place with the result — no side
panel, no scratch file. Decoding also recognises base64url and JWT-shaped
strings, so pasting a token and expanding it into readable JSON is one
keystroke.

## Features

- **Encode the selection.** The selected text is read as UTF-8 and replaced
  with standard Base64 — the `A–Z a–z 0–9 + /` alphabet with `=` padding. The
  selection is encoded exactly as it stands, whitespace and newlines included;
  nothing is trimmed on the way in.

- **Decode the selection**, accepting more than one spelling. Leading and
  trailing whitespace is trimmed first, then the text is matched against
  standard Base64 (`+`, `/`, and zero to two trailing `=` — padding is
  optional) or against base64url (`-` and `_`, padding added back for you).
  The bytes are read back as UTF-8.

- **JWT-shaped strings are expanded, not just decoded.** If the selection
  splits on `.` into two or more base64url parts, each part is decoded
  separately; any part that parses as JSON is re-printed with two-space
  indentation. For a three-part token the last part — the signature, which is
  binary, not text — is left untouched.

- **Right-click and keyboard access.** Both commands appear in the editor
  context menu whenever there is a selection, and both have chord keybindings.

Both commands act on the primary selection of the active editor. If nothing is
selected you get a `Base64 Tools: No text selected.` warning, and if the
selection is not valid Base64 you get `Base64 Tools: Selection is not valid
base64.` — in neither case is the document touched.

## Encoding

Select `Hello, World!` and run **Base64: Encode Selection to Base64**. The
selection becomes:

```
SGVsbG8sIFdvcmxkIQ==
```

Encoding always emits the standard alphabet with padding. There is no
base64url encode command; decoding accepts base64url, encoding does not
produce it.

## Decoding

Running **Base64: Decode Selection from Base64** reverses that. Padding may be
missing, and surrounding whitespace is ignored, so all three of these decode to
`Hello`:

```
SGVsbG8=
SGVsbG8
  SGVsbG8=
```

Whitespace *inside* the string is not stripped, so a wrapped or space-separated
blob like `SGVs bG8=` is rejected rather than silently repaired.

base64url input works too — `SGVsbG8-V29ybGQ_` decodes to `Hello>World?`.

## JWT-shaped strings

Decoding checks for a dot-separated shape before it tries plain Base64. Select
a token like:

```
eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c
```

and decoding replaces it with:

```
{
  "alg": "HS256",
  "typ": "JWT"
}
.
{
  "sub": "1234567890",
  "name": "John Doe",
  "iat": 1516239022
}
.
SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c
```

The parts are rejoined with a `.` on its own line, so the structure of the
original token is still legible. Two-part strings (a header and payload with no
signature) are handled the same way, with both parts decoded. A part that does
not contain JSON is left as its decoded text rather than being reformatted.

This is a decoder, not a validator: the signature is copied through verbatim
and nothing is verified. Do not treat a decoded token as a trusted one.

## Commands

Both are available from the Command Palette and from the editor context menu
(shown only when text is selected).

| Command | Command ID |
| --- | --- |
| **Base64: Encode Selection to Base64** | `base64-tools.encode` |
| **Base64: Decode Selection from Base64** | `base64-tools.decode` |

## Keybindings

Both are chords, and both are active only when the editor has focus and a
selection.

| Command | Windows / Linux | macOS |
| --- | --- | --- |
| Encode | `ctrl+e ctrl+e` | `cmd+e cmd+e` |
| Decode | `ctrl+e ctrl+d` | `cmd+e cmd+d` |

## Settings

This extension contributes no settings.
