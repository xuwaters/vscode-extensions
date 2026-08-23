# 0003 — Write the template parser rather than bind an existing one

**Status**: Accepted, gated on [P1-05](../tasks/phase-1-foundation.md) · **Date**: 2026-08-22

## Context

fast-analyzer parses templates with `parse5.parseFragment(html, { sourceCodeLocationInfo: true })`.
In Rust the candidates are:

| Option | Spans | Tolerant | Notes |
| --- | --- | --- | --- |
| `html5ever` | Line/column via the tokenizer; the tree builder does not carry them | Spec-compliant, which here means *it fixes your markup* | Already in this workspace's `Cargo.lock`, transitively |
| `swc_html_parser` | Full `BytePos` spans on elements, attributes, values | Yes, with recovered errors | Heavy dependency tree |
| Our own | Whatever we define | Whatever we define | ~1,500 lines, and HTML is fiddly |

## Decision

Write it, as `fast-template-syntax`, with `swc_html_parser` as the named fallback.

## Why

Four requirements, and the first two rule out an off-the-shelf HTML5 parser regardless of quality:

1. **Unclosed tags must stay unclosed.** A spec-compliant parser auto-closes them — that is what the
   specification says to do. `no-unclosed-tag` is the second-highest-severity rule we ship
   (`warn`/`error`) and it needs to see what was written, not what the parser repaired.
2. **The modifier character needs its own span.** `?disabled` should be diagnosable as "the `?` is
   wrong here", not as "the attribute `?disabled` is unknown". No HTML parser has a concept of an
   attribute-name prefix, because HTML does not.
3. **Placeholders appear in attribute-name position** — `<div ${ref('tableEl')}>`, 17 times in the
   corpus. The virtual document's substitution keeps that parseable
   ([architecture.md §3](../design/architecture.md#3-the-virtual-document)), but the tree has to
   record *which* expression an attribute came from, and that is not a concept an HTML parser has
   either.
4. **Spans on everything, indexing into the caller's string.** No copies, no line/column
   reconstruction — the whole coordinate model depends on byte offsets being exact.

## Consequences

**We take on HTML's fiddly parts** and can get them wrong: raw-text elements (`<style>`, `<script>`,
`<textarea>`, `<title>`), foreign content where `<path/>` self-closing is legal, implied end tags
(`<li>`, `<p>`, `<tr>`), and attribute values containing `<` or `>`. The corpus is full of inline
SVG — 12 self-closing tags in csv-ultra's template alone — so foreign content is not an edge case
here.

**The gate is a differential test, not a review.** Our tree and every span, against parse5's, over:
the 26 corpus templates, lit-analyzer's own parser fixtures, and a hand-written adversarial set.
Deliberate divergences — unclosed tags, chiefly — are enumerated in the harness as expected
differences rather than allowed silently, so a *new* divergence is a failure.
[gate 2](../research/spikes.md#gate-2--does-our-parser-match-parse5).

**If the gate fails**, switch to `swc_html_parser` and record a superseding ADR. It has spans and
recovery; what it does not have is requirement 1, so that would come back as a post-pass over the
recovered errors — worse, but workable.

**The crate knows nothing about tagged templates.** It takes text and a placeholder table. That is
what would let declarative `.html` FAST templates
([research/fast-element.md §11](../research/fast-element.md#11-declarative-templates-out-of-scope))
reuse it without a second parser.

## Revisit if

- The differential test finds divergences we cannot close in the time budgeted for P1-05.
- Something in the tree turns out to need full HTML5 tree construction — foreign-content breakout,
  or table-content fostering. Neither can occur in a FAST template that a browser would render as
  written, but "cannot occur" is a claim the differential test is there to check.
