NO LICENSE

---

# Third-Party Notices

This extension's published package (`dist/`, `wasm/`) contains third-party
software redistributed under the licenses below. Each copyright holder retains
their rights; nothing here grants any license to this extension itself.

## Fonts

The KaTeX font files shipped at `dist/webview/fonts/KaTeX_*.{woff2,woff,ttf}`
come from the [KaTeX/katex-fonts](https://github.com/KaTeX/katex-fonts) project
and are redistributed under the MIT license:

> Copyright (c) 2018 Khan Academy

`dist/webview/style.css` contains the KaTeX `@font-face` rules that reference
them, from the `katex` package itself:

> Copyright (c) 2013-2020 Khan Academy and other contributors

Both are covered by the [MIT License](#mit-license) text below.

## Bundled JavaScript (`dist/`)

### MIT

| Package | Copyright |
| --- | --- |
| katex | Copyright (c) 2013-2020 Khan Academy and other contributors |
| mermaid | Copyright (c) 2014-2022 Knut Sveidqvist |
| @mermaid-js/parser | Copyright (c) 2023 Yokozuna59 |
| @braintree/sanitize-url | Copyright (c) 2017 Braintree |
| @iconify/utils | Copyright (c) 2021-present Vjacheslav Trushkin |
| @upsetjs/venn.js | Copyright (c) 2013 Ben Frederickson; Copyright (c) 2021 Samuel Gratzl |
| chevrotain-allstar | Copyright 2022 TypeFox GmbH |
| langium | Copyright 2021 TypeFox GmbH |
| cose-base | Copyright (c) 2019-present, iVis@Bilkent |
| layout-base | Copyright (c) 2019 iVis@Bilkent |
| cytoscape | Copyright (c) 2016-2026, The Cytoscape Consortium |
| cytoscape-cose-bilkent | Copyright (c) 2016-2018, The Cytoscape Consortium |
| cytoscape-fcose | Copyright (c) 2018-present, iVis-at-Bilkent |
| dagre-d3-es | Copyright (c) 2013 Chris Pettitt; Copyright (c) 2012-2014 Chris Pettitt |
| dayjs | Copyright (c) 2018-present, iamkun |
| js-yaml | Copyright (C) 2011-2015 by Vitaly Puzrin |
| khroma | Copyright (c) 2019-present Fabio Spampinato, Andrew Maney |
| lodash-es | Copyright OpenJS Foundation and other contributors |
| marked | Copyright (c) 2018+ MarkedJS; Copyright (c) 2011-2018 Christopher Jeffrey |
| roughjs | Copyright (c) 2019 Preet Shihn |
| stylis | Copyright (c) 2016-present Sultan Tarimo |
| ts-dedent | Copyright (c) 2018 Tamino Martinius |
| uuid | Copyright (c) 2010-2020 Robert Kieffer and other contributors |
| vscode-jsonrpc, vscode-languageserver-protocol, vscode-languageserver-textdocument, vscode-languageserver-types, vscode-uri | Copyright (c) Microsoft Corporation |

### BSD 3-Clause

| Package | Copyright |
| --- | --- |
| highlight.js | Copyright (c) 2006, Ivan Sagalaev |
| d3-array | Copyright 2010-2020 Mike Bostock |
| d3-ease, d3-path, d3-sankey, d3-shape | Copyright Mike Bostock |

### ISC

Copyright Mike Bostock — d3-axis, d3-brush, d3-color, d3-dispatch, d3-format,
d3-hierarchy, d3-interpolate, d3-scale, d3-scale-chromatic, d3-selection,
d3-time, d3-time-format, d3-timer, d3-transition, d3-zoom, internmap.

### Apache-2.0

chevrotain, @chevrotain/gast, @chevrotain/regexp-to-ast, @chevrotain/utils —
see <https://www.apache.org/licenses/LICENSE-2.0>.

### MPL-2.0 OR Apache-2.0

dompurify — Copyright 2025 Dr.-Ing. Mario Heiderich, Cure53. Used under
Apache-2.0. See <https://github.com/cure53/DOMPurify>.

## Bundled WebAssembly (`wasm/markdown_engine_bg.wasm`)

Compiled from the `markdown-engine` crate; the following Rust crates are
statically linked into the binary.

- **MIT OR Apache-2.0** — ammonia, bumpalo, cfg-if, displaydoc, dtoa,
  form_urlencoded, hashbrown, html5ever, idna, itoa, jetscii, libc, lock_api,
  log, once_cell, parking_lot, parking_lot_core, percent-encoding, proc-macro2,
  quote, rustc-hash, scopeguard, serde, serde_core, serde_derive, serde_json,
  serde_yaml, smallvec, stable_deref_trait, string_cache, syn, tendril,
  unicode-normalization, url, wasm-bindgen (and macro crates), web_atoms,
  maplit, siphasher, console_error_panic_hook, equivalent, idna_adapter,
  indexmap, utf8_iter, tinyvec, tinyvec_macros, memchr, ryu
- **MIT** — caseless, new_debug_unreachable, phf, phf_shared,
  precomputed-hash, synstructure, typed-arena, unsafe-libyaml, zmij
- **BSD-2-Clause** — comrak
- **Apache-2.0** — similar
- **Unicode-3.0** — the ICU4X crates (icu_collections, icu_locale_core,
  icu_normalizer(+_data), icu_properties(+_data), icu_provider, litemap,
  potential_utf, tinystr, writeable, yoke(+-derive), zerofrom(+-derive),
  zerotrie, zerovec(+-derive)). See
  <https://www.unicode.org/license.txt>.
- **MPL-2.0** — cssparser, dtoa-short (pulled in by `ammonia`). MPL-2.0
  requires that the source of these files be made available to recipients of
  the binary; unmodified upstream sources are at
  <https://github.com/servo/rust-cssparser> and
  <https://github.com/upsuper/dtoa-short>.

## MIT License

```
Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## BSD 3-Clause License

```
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

* Redistributions of source code must retain the above copyright notice, this
  list of conditions and the following disclaimer.

* Redistributions in binary form must reproduce the above copyright notice,
  this list of conditions and the following disclaimer in the documentation
  and/or other materials provided with the distribution.

* Neither the name of the copyright holder nor the names of its
  contributors may be used to endorse or promote products derived from
  this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## BSD 2-Clause License

```
Redistribution and use in source and binary forms, with or without
modification, are permitted provided that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this
   list of conditions and the following disclaimer.

2. Redistributions in binary form must reproduce the above copyright notice,
   this list of conditions and the following disclaimer in the documentation
   and/or other materials provided with the distribution.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
```

## ISC License

```
Permission to use, copy, modify, and/or distribute this software for any
purpose with or without fee is hereby granted, provided that the above
copyright notice and this permission notice appear in all copies.

THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES WITH
REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF MERCHANTABILITY
AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR ANY SPECIAL, DIRECT,
INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES WHATSOEVER RESULTING FROM
LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION OF CONTRACT, NEGLIGENCE OR
OTHER TORTIOUS ACTION, ARISING OUT OF OR IN CONNECTION WITH THE USE OR
PERFORMANCE OF THIS SOFTWARE.
```
