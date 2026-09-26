MIT License

Copyright (c) 2026 Wei Xu

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

---

# Third-Party Notices

The license above covers this extension's own code. The published package also
redistributes other people's work, which stays under its own terms: those terms
are not replaced by the MIT License, and the MIT License does not extend to
them.

## 1. PDF.js

`dist/webview.js` and `dist/pdf.worker.js` bundle `pdfjs-dist` 6.2.108
(<https://github.com/mozilla/pdf.js>), redistributed under the Apache License
2.0:

> Copyright 2024 Mozilla Foundation

The full text of the Apache License 2.0 ships at `dist/pdfjs/LICENSE`, copied
unchanged from the package.

`dist/pdfjs/` also carries the data pdf.js reads at runtime, copied unchanged
from the same package. Each part has its own terms, and the upstream license
file for each ships beside it:

| Files | Covers | License | Notice shipped at |
| --- | --- | --- | --- |
| `cmaps/*.bcmap` | CJK character maps | BSD-3-Clause, Copyright 1990-2009 Adobe Systems Incorporated | `dist/pdfjs/cmaps/LICENSE` |
| `standard_fonts/Foxit*.pfb` | The standard 14 fonts | BSD-3-Clause, Copyright 2014 PDFium Authors | `dist/pdfjs/standard_fonts/LICENSE_FOXIT` |
| `standard_fonts/LiberationSans-*.ttf` | Sans-serif fallback | SIL Open Font License 1.1, Copyright (c) 2010 Google Corporation, Copyright (c) 2012 Red Hat, Inc. | `dist/pdfjs/standard_fonts/LICENSE_LIBERATION` |
| `wasm/openjpeg.wasm`, `wasm/openjpeg_nowasm_fallback.js` | JPEG 2000 decoding | BSD-2-Clause (OpenJPEG authors; Mozilla Foundation for the build) | `dist/pdfjs/wasm/LICENSE_OPENJPEG`, `LICENSE_PDFJS_OPENJPEG` |
| `wasm/jbig2.wasm`, `wasm/jbig2_nowasm_fallback.js` | JBIG2 decoding | BSD-3-Clause, Copyright 2014 The PDFium Authors; Apache-2.0, Copyright 2026 Mozilla Foundation, for the build | `dist/pdfjs/wasm/LICENSE_JBIG2`, `LICENSE_PDFJS_JBIG2` |
| `wasm/qcms_bg.wasm` | ICC colour conversion | MIT, Copyright (C) 2009-2024 Mozilla Corporation, Copyright (C) 1998-2007 Marti Maria; BSD-2-Clause, Copyright (c) 2025 Mozilla Foundation, for the build | `dist/pdfjs/wasm/LICENSE_QCMS`, `LICENSE_PDFJS_QCMS` |

## 2. FAST Element

`dist/webview.js` also includes `@microsoft/fast-element` 3.0.2
(<https://github.com/microsoft/fast>), redistributed under the MIT License:

> Copyright (c) Microsoft Corporation.

Its permission notice is the same text as the MIT License at the top of this
file, with Microsoft Corporation as the copyright holder.
