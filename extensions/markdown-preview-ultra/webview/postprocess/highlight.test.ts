// @vitest-environment happy-dom
import hljs from 'highlight.js/lib/common';
import { describe, expect, it } from 'vitest';
import { highlightCode } from './highlight';

/** Importing highlight.ts registers the extra languages on the hljs singleton. */
function highlight(source: string, language: string): string {
  return hljs.highlight(source, { language, ignoreIllegals: true }).value;
}

const SCHEMA = `@0xdbb9ad1f14bf0b36;

using Cxx = import "/capnp/c++.capnp";
$Cxx.namespace("addressbook");

# A person in the address book.
struct Person {
  id @0 :UInt32;
  name @1 :Text;
  phones @2 :List(PhoneNumber);

  employment :union {
    unemployed @3 :Void;
    employer @4 :Text;
  }

  enum Kind {
    mobile @0;
    home @1;
  }
}

interface Directory extends(Node) {
  lookup @0 (name :Text) -> (person :Person);
  watch @1 () -> stream;
}

const pi :Float64 = 3.14159;
const raw :Data = 0x"62 61 72";
`;

describe('capnp grammar', () => {
  const html = highlight(SCHEMA, 'capnp');

  it('is registered under its name and aliases', () => {
    expect(hljs.getLanguage('capnp')).toBeTruthy();
    expect(hljs.getLanguage('capnproto')).toBeTruthy();
    expect(hljs.getLanguage('capn-proto')).toBeTruthy();
  });

  it('names declarations', () => {
    expect(html).toContain(
      '<span class="hljs-keyword">struct</span> <span class="hljs-title class_">Person</span>',
    );
    expect(html).toContain(
      '<span class="hljs-keyword">interface</span> <span class="hljs-title class_">Directory</span>',
    );
    expect(html).toContain(
      '<span class="hljs-keyword">enum</span> <span class="hljs-title class_">Kind</span>',
    );
    expect(html).toContain(
      '<span class="hljs-keyword">using</span> <span class="hljs-title class_">Cxx</span>',
    );
    expect(html).toContain(
      '<span class="hljs-keyword">const</span> <span class="hljs-title">pi</span>',
    );
  });

  it('marks the file id and ordinals as symbols', () => {
    expect(html).toContain(
      '<span class="hljs-symbol">@0xdbb9ad1f14bf0b36</span>',
    );
    expect(html).toContain(
      '<span class="hljs-attribute">id</span> <span class="hljs-symbol">@0</span>',
    );
  });

  it('separates methods from fields', () => {
    expect(html).toContain(
      '<span class="hljs-title function_">lookup</span> <span class="hljs-symbol">@0</span>',
    );
    expect(html).toContain(
      '<span class="hljs-attribute">name</span> <span class="hljs-symbol">@1</span>',
    );
  });

  it('highlights types, keywords, literals and comments', () => {
    expect(html).toContain('<span class="hljs-built_in">UInt32</span>');
    expect(html).toContain('<span class="hljs-built_in">List</span>');
    expect(html).toContain('<span class="hljs-title class_">PhoneNumber</span>');
    expect(html).toContain('<span class="hljs-keyword">union</span>');
    expect(html).toContain('<span class="hljs-keyword">extends</span>');
    expect(html).toContain('<span class="hljs-keyword">stream</span>');
    expect(html).toContain('<span class="hljs-keyword">-&gt;</span>');
    expect(html).toContain(
      '<span class="hljs-comment"># A person in the address book.</span>',
    );
  });

  it('highlights annotations, strings, data and numbers', () => {
    expect(html).toContain('<span class="hljs-meta">$Cxx.namespace</span>');
    expect(html).toContain(
      '<span class="hljs-string">&quot;/capnp/c++.capnp&quot;</span>',
    );
    expect(html).toContain(
      '<span class="hljs-string">0x&quot;62 61 72&quot;</span>',
    );
    expect(html).toContain('<span class="hljs-number">3.14159</span>');
  });
});

describe('highlightCode', () => {
  function render(html: string): HTMLElement {
    document.body.innerHTML = `<div id="c">${html}</div>`;
    const root = document.getElementById('c') as HTMLElement;
    highlightCode([root]);
    return root;
  }

  it('highlights a capnp fence', () => {
    const root = render(
      '<pre><code class="language-capnp">struct Foo {}</code></pre>',
    );
    const code = root.querySelector('code') as HTMLElement;
    expect(code.classList.contains('hljs')).toBe(true);
    expect(code.innerHTML).toContain(
      '<span class="hljs-keyword">struct</span> <span class="hljs-title class_">Foo</span>',
    );
  });

  it('highlights the capnproto alias too', () => {
    const root = render(
      '<pre><code class="language-capnproto">const pi :Float64 = 3.0;</code></pre>',
    );
    const code = root.querySelector('code') as HTMLElement;
    expect(code.innerHTML).toContain('<span class="hljs-built_in">Float64</span>');
  });

  it('leaves math fences alone', () => {
    const root = render(
      '<pre><code class="language-math" data-math-style="display">x^2</code></pre>',
    );
    const code = root.querySelector('code') as HTMLElement;
    expect(code.classList.contains('hljs')).toBe(false);
    expect(code.innerHTML).toBe('x^2');
  });
});
