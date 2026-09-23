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

const PRISMA = `// Prisma schema
datasource db {
  provider = "postgresql"
  url      = env("DATABASE_URL")
}

generator client {
  provider        = "prisma-client-js"
  previewFeatures = ["views"]
}

/// A registered user.
model User {
  id        Int      @id @default(autoincrement())
  email     String   @unique @db.VarChar(255)
  type      Role     @default(USER)
  posts     Post[]
  profile   profiles?
  createdAt DateTime @default(now())
  active    Boolean  @default(true)

  @@index([email(sort: Desc)])
  @@map("users")
}

model Post {
  id       Int  @id
  author   User @relation(fields: [authorId], references: [id], onDelete: Cascade)
  authorId Int
}

enum Role {
  USER
  ADMIN @map("admin")
}

type Address {
  street String
}
`;

describe('prisma grammar', () => {
  const html = highlight(PRISMA, 'prisma');

  it('is registered', () => {
    expect(hljs.getLanguage('prisma')).toBeTruthy();
  });

  it('names blocks', () => {
    expect(html).toContain(
      '<span class="hljs-keyword">model</span> <span class="hljs-title class_">User</span>',
    );
    expect(html).toContain(
      '<span class="hljs-keyword">enum</span> <span class="hljs-title class_">Role</span>',
    );
    expect(html).toContain(
      '<span class="hljs-keyword">type</span> <span class="hljs-title class_">Address</span>',
    );
    expect(html).toContain(
      '<span class="hljs-keyword">datasource</span> <span class="hljs-title">db</span>',
    );
    expect(html).toContain(
      '<span class="hljs-keyword">generator</span> <span class="hljs-title">client</span>',
    );
  });

  it('highlights config keys and values', () => {
    expect(html).toContain('<span class="hljs-attr">provider</span>');
    expect(html).toContain('<span class="hljs-attr">previewFeatures</span>');
    expect(html).toContain('<span class="hljs-built_in">env</span>');
    expect(html).toContain(
      '<span class="hljs-string">&quot;DATABASE_URL&quot;</span>',
    );
  });

  it('pairs field names with their types', () => {
    expect(html).toContain(
      '<span class="hljs-attribute">id</span>        <span class="hljs-built_in">Int</span>',
    );
    expect(html).toContain(
      '<span class="hljs-attribute">posts</span>     <span class="hljs-title class_">Post</span>[]',
    );
    // Lowercase model names are still types by position.
    expect(html).toContain(
      '<span class="hljs-attribute">profile</span>   <span class="hljs-title class_">profiles</span>?',
    );
    // A field named like a block keyword stays a field.
    expect(html).toContain(
      '<span class="hljs-attribute">type</span>      <span class="hljs-title class_">Role</span>',
    );
  });

  it('highlights attributes and their arguments', () => {
    expect(html).toContain('<span class="hljs-meta">@id</span>');
    expect(html).toContain('<span class="hljs-meta">@db.VarChar</span>(<span class="hljs-number">255</span>)');
    expect(html).toContain('<span class="hljs-meta">@@index</span>');
    expect(html).toContain('<span class="hljs-built_in">autoincrement</span>');
    expect(html).toContain('<span class="hljs-attr">fields</span>');
    expect(html).toContain('<span class="hljs-attr">onDelete</span>');
    expect(html).toContain('<span class="hljs-literal">Cascade</span>');
    expect(html).toContain('<span class="hljs-literal">Desc</span>');
    expect(html).toContain('<span class="hljs-literal">true</span>');
  });

  it('highlights enumerants and comments', () => {
    expect(html).toContain('<span class="hljs-variable constant_">USER</span>');
    expect(html).toContain(
      '<span class="hljs-variable constant_">ADMIN</span> <span class="hljs-meta">@map</span>',
    );
    expect(html).toContain('<span class="hljs-comment">/// A registered user.</span>');
  });

  it('highlights a bare excerpt of fields', () => {
    const excerpt = highlight('  email String @unique // login', 'prisma');
    expect(excerpt).toContain(
      '<span class="hljs-attribute">email</span> <span class="hljs-built_in">String</span> <span class="hljs-meta">@unique</span>',
    );
    expect(excerpt).toContain('<span class="hljs-comment">// login</span>');
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
