# Askama Templates

Syntax highlighting and snippets for [Askama](https://github.com/askama-rs/askama),
the Jinja-like template engine for Rust. Templates keep their host language's
highlighting — HTML stays HTML, YAML stays YAML — and the Askama tags layered on
top get their own colours so `{% … %}`, `{{ … }}` and `{# … #}` stand out
wherever they appear.

## Features

- **15 template languages**, one per host format: HTML, plain text, Rust,
  JavaScript, TypeScript, JSX, TSX, TOML, CSS, JSON, Markdown, YAML,
  `.gitignore`, dotenv and Swift. Each is claimed by a base extension plus one
  of four template suffixes — `.askama`, `.j2`, `.jinja`, `.jinja2` — so
  `page.html.askama`, `page.html.j2`, `page.html.jinja` and `page.html.jinja2`
  all resolve to Askama HTML.

- **Host grammar underneath.** Every language grammar includes the real
  upstream grammar (`text.html.basic`, `source.rust`, `source.yaml`,
  `text.html.markdown`, `source.toml`, and so on) after the Askama patterns, so
  tags, attributes, keys and code around the template tags are highlighted the
  way they normally are. Formats whose grammar ships in another extension —
  TOML, dotenv, `.gitignore` — fall back to plain text for the host part if that
  extension is not installed; the Askama tags are highlighted either way.

- **Full tag grammar**: statements, expressions and comments including all
  whitespace-control markers (`{%-`, `{%+`, `{%~` and their closing forms), 30
  control keywords (`if`/`elif`/`else`, `for`/`in`, `match`/`when`,
  `block`, `extends`, `include`, `import`, `macro`, `call`, `filter`, `let`,
  `set`, `mut`, `decl`, `declare`, `continue`, `break` and their `end…`
  partners), single- and double-quoted strings with escapes, decimal, hex,
  octal, binary and float literals, booleans, filter pipes (`| safe`),
  `loop.index`, `loop.index0`, `loop.first`, `loop.last`, the `self` / `Self` /
  `caller` / `crate` variables, macro calls like `format!(…)`, and the operator
  set Askama actually has: `==`, `!=`, `<=`, `>=`, `&&`, `||`, `!`, `bitand`,
  `bitor`, `xor`, `as`, `not`, `is defined` / `is not defined`, range `..` and
  concat `~`.

- **`{% raw %}` is respected.** Text between `{% raw %}` and `{% endraw %}` is
  highlighted as host-language content only; template syntax inside it is left
  alone, which is the point of the block.

- **Dedicated template colours.** On top of the grammar, the extension paints
  template regions with seven themable colours, so delimiters, keywords,
  expression content, comments, filters, function calls and named arguments
  read consistently no matter which theme or host language you are in. The
  colours have separate defaults for dark, light, high contrast and high
  contrast light themes, and repaint 50 ms after you stop typing.

- **16 snippets**, available in all 15 languages. Each block tag has both a
  bare prefix and a `%`-prefixed one — `for` and `%for` both expand to a
  `{% for … %} … {% endfor %}` pair with tab stops on the loop variable and the
  iterable. `{{` expands to `{{ expr }}` and `{#` to `{# comment #}`.

- **Editing niceties** from the language configuration: `{#` / `#}` comment
  toggling, auto-closing and surrounding for `{%…%}`, `{{…}}`, `{#…#}` as well
  as quotes and the usual brackets, bracket-pair colourization for `{%…%}` and
  `{{…}}`, and indentation rules that indent after an opening `if`, `elif`,
  `else`, `for`, `block`, `macro`, `call`, `filter`, `match`, `when` or an HTML
  open tag and dedent on the matching `end…` tag or closing tag.

## File names

| Language | Base name | Full example |
| --- | --- | --- |
| Askama HTML | `.html`, `.htm` | `index.html.askama` |
| Askama Text | `.txt`, or the suffix alone | `mail.txt.j2`, `notes.jinja` |
| Askama Rust | `.rs` | `handler.rs.askama` |
| Askama JavaScript | `.js` | `app.js.jinja2` |
| Askama TypeScript | `.ts` | `client.ts.askama` |
| Askama JSX | `.jsx` | `page.jsx.j2` |
| Askama TSX | `.tsx` | `page.tsx.askama` |
| Askama TOML | `.toml` | `Cargo.toml.jinja` |
| Askama CSS | `.css` | `theme.css.askama` |
| Askama JSON | `.json` | `config.json.j2` |
| Askama Markdown | `.md`, `.markdown` | `README.md.askama` |
| Askama YAML | `.yaml`, `.yml` | `deploy.yaml.jinja2` |
| Askama gitignore | `.gitignore` | `.gitignore.askama`, `gitignore.j2` |
| Askama dotenv | `.env` | `.env.askama`, `.env.production.j2` |
| Askama Swift | `.swift` | `View.swift.askama` |

Every base name above works with all four suffixes. The dotenv language also
matches `.env.*.askama` and the `.j2` / `.jinja` / `.jinja2` variants, so
per-environment files like `.env.staging.jinja` are picked up too.

## Example

`page.html.askama` — HTML and Askama highlighted together:

```jinja
{% extends "base.html" %}

{% block content %}
<div class="container">
  {% if user.is_authenticated %}
    <h1>Welcome, {{ user.name | capitalize }}!</h1>
  {% elif visitor_count > 100 %}
    <h1>Welcome, visitor #{{ visitor_count }}!</h1>
  {% endif %}

  <ul>
    {% for item in items %}
      <li class="{% if loop.first %}first{% endif %}">
        {{ loop.index }}. {{ item.name | truncate(30) }}
      </li>
    {% endfor %}
  </ul>

  {% match status %}
    {% when Status::Active %}<span class="badge">Active</span>
    {% when _ %}<span class="badge">Unknown</span>
  {% endmatch %}

  {% let full_name = format!("{} {}", first_name, last_name) %}

  {% raw %}
    <p>This {{ stays_literal }} — no template processing here.</p>
  {% endraw %}

  {% import "macros.html" as m %}
  {{ m.render_card(title="Hello", body="World") }}
</div>
{% endblock %}
```

The same tags work in any of the other 14 languages; only the surrounding
grammar changes. In `deploy.yaml.j2` the YAML keys stay YAML, in
`handler.rs.jinja` the Rust stays Rust.

## Colors

The extension contributes no settings and no commands. It does contribute seven
theme colours, which you can override per theme in `workbench.colorCustomizations`:

| Color id | Dark default | Description |
| --- | --- | --- |
| `askamaTemplates.delimiterForeground` | `#d4976c` | Template delimiters (`{%`, `%}`, `{{`, `}}`, `{#`, `#}`) |
| `askamaTemplates.keywordForeground` | `#c9874e` | Template keywords (`if`, `for`, `block`, …) |
| `askamaTemplates.contentForeground` | `#e0c285` | Expressions and variables |
| `askamaTemplates.commentForeground` | `#a0855b` | Template comments |
| `askamaTemplates.filterForeground` | `#d4a888` | Filters (`\| capitalize`, `\| safe`, …) |
| `askamaTemplates.functionForeground` | `#d4a888` | Function and method calls in expressions |
| `askamaTemplates.namedArgForeground` | `#b0a098` | Named argument keys (`title=`, `body=`) |

Each also has light, high contrast and high contrast light defaults. Delimiters
and keywords are drawn bold, comments italic.

```jsonc
"workbench.colorCustomizations": {
  "askamaTemplates.delimiterForeground": "#7aa2f7"
}
```
