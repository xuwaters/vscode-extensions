import * as fs from 'node:fs';
import * as path from 'node:path';
import { beforeAll, describe, expect, it } from 'vitest';
import * as oniguruma from 'vscode-oniguruma';
import * as textmate from 'vscode-textmate';

/**
 * The injection grammar, tokenized for real.
 *
 * The grammar only ever runs injected into VS Code's own TypeScript grammar,
 * against the real `text.html.basic` and `source.css` — so that is what this
 * suite runs it against. A JavaScript `RegExp` cannot stand in for Oniguruma,
 * and a hand-written stub for `source.ts` cannot reproduce the thing that
 * actually decides whether the injection fires: which of the two grammars wins
 * at the character where a tagged template begins.
 *
 * Skipped where no VS Code install is found (CI's node job), the same
 * arrangement as the WASM suites.
 */

const ROOT = path.join(__dirname, '..');

const APP_EXTENSIONS = [
  '/Applications/Visual Studio Code.app/Contents/Resources/app/extensions',
  '/Applications/Visual Studio Code - Insiders.app/Contents/Resources/app/extensions',
  '/Applications/Cursor.app/Contents/Resources/app/extensions',
  '/usr/share/code/resources/app/extensions',
].find(candidate => fs.existsSync(candidate));

/** TextMate scope → grammar file, read from the built-in extensions' manifests. */
function readBuiltins(directory: string): Map<string, string> {
  const grammars = new Map<string, string>();
  for (const name of fs.readdirSync(directory)) {
    const manifest = path.join(directory, name, 'package.json');
    if (!fs.existsSync(manifest)) continue;
    let contributes;
    try {
      contributes = JSON.parse(fs.readFileSync(manifest, 'utf8')).contributes;
    } catch {
      continue;
    }
    for (const grammar of contributes?.grammars ?? []) {
      grammars.set(grammar.scopeName, path.join(directory, name, grammar.path));
    }
  }
  return grammars;
}

interface GrammarContribution {
  scopeName: string;
  path: string;
  injectTo: string[];
}

/** Every grammar the manifest contributes, and where it says to inject it. */
const OURS: GrammarContribution[] = JSON.parse(
  fs.readFileSync(path.join(ROOT, 'package.json'), 'utf8'),
).contributes.grammars;

let registry: textmate.Registry | undefined;

beforeAll(async () => {
  if (!APP_EXTENSIONS) return;
  const builtins = readBuiltins(APP_EXTENSIONS);

  const wasm = path.join(ROOT, 'node_modules', 'vscode-oniguruma', 'release', 'onig.wasm');
  await oniguruma.loadWASM(fs.readFileSync(wasm).buffer as ArrayBuffer);

  registry = new textmate.Registry({
    onigLib: Promise.resolve({
      createOnigScanner: sources => new oniguruma.OnigScanner(sources),
      createOnigString: source => new oniguruma.OnigString(source),
    }),
    // Exactly what `contributes.grammars[].injectTo` asks VS Code to do.
    getInjections: scopeName => {
      const injected = OURS.filter(g => g.injectTo.includes(scopeName)).map(g => g.scopeName);
      return injected.length ? injected : undefined;
    },
    loadGrammar: async scopeName => {
      const own = OURS.find(g => g.scopeName === scopeName);
      const file = own ? path.join(ROOT, own.path) : builtins.get(scopeName);
      if (!file) return null;
      return textmate.parseRawGrammar(fs.readFileSync(file, 'utf8'), file);
    },
  });
});

/** Every token of `source`, tokenized as TypeScript, one array per line. */
async function tokenize(source: string): Promise<textmate.IToken[][]> {
  const grammar = await registry!.loadGrammar('source.ts');
  expect(grammar, 'source.ts did not load').not.toBeNull();

  let stack = textmate.INITIAL;
  const lines: textmate.IToken[][] = [];
  for (const line of source.split('\n')) {
    const result = grammar!.tokenizeLine(line, stack);
    stack = result.ruleStack;
    lines.push(result.tokens);
  }
  return lines;
}

/** Scopes on the token covering `needle`, wherever in `source` it first is. */
async function scopesAt(source: string, needle: string): Promise<string[]> {
  const lines = source.split('\n');
  const row = lines.findIndex(line => line.includes(needle));
  expect(row, `${JSON.stringify(needle)} is not in the source`).toBeGreaterThanOrEqual(0);

  const column = lines[row].indexOf(needle);
  const tokens = (await tokenize(source))[row];
  const token = tokens.find(t => t.startIndex <= column && column < t.endIndex);
  expect(token, `no token at ${row}:${column}`).toBeDefined();
  return token!.scopes;
}

const describeWithCode = APP_EXTENSIONS ? describe : describe.skip;

describeWithCode('html`` templates', () => {
  it('hands an untyped template to the HTML grammar', async () => {
    const scopes = await scopesAt('const t = html`<div class="a">hi</div>`;', 'div');
    expect(scopes).toContain('meta.embedded.block.html');
    expect(scopes).toContain('entity.name.tag.html');
  });

  it('hands a typed html<T>`` template to the HTML grammar', async () => {
    const scopes = await scopesAt('const t = html<Foo>`<div class="a">hi</div>`;', 'div');
    expect(scopes).toContain('meta.embedded.block.html');
    expect(scopes).toContain('entity.name.tag.html');
  });

  it('gives the type arguments the colours they have anywhere else', async () => {
    // The opener must not swallow them: text a `begin` matches but does not
    // capture takes the rule's own `name`, which would paint them as string.
    // `AtMessage` would be repaired by tsserver's semantic tokens; `string`,
    // which has none of its own, would stay wrong.
    const source = 'const t = html<string, AtMessage>`<button>hi</button>`;';
    expect(await scopesAt(source, 'string')).toContain('support.type.primitive.ts');
    expect(await scopesAt(source, 'AtMessage')).toContain('entity.name.type.ts');
    expect(await scopesAt(source, 'string')).not.toContain('string.template.fast-element.ts');
    expect(await scopesAt(source, 'button')).toContain('entity.name.tag.html');
  });

  it('scopes attribute names inside a typed template', async () => {
    const source = 'const t = html<Foo>`<div class="mc-doc">hi</div>`;';
    expect(await scopesAt(source, 'class')).toContain('entity.other.attribute-name.html');
    expect(await scopesAt(source, '"mc-doc"')).toContain('string.quoted.double.html');
  });

  it('survives a multi-line template opened on a continuation line', async () => {
    const source = [
      'export const template = when(',
      '  x => x.ready,',
      '  html<McFilePreview>`',
      '    <div class="mc-lightbox__doc">',
      '      <span class="mc-lightbox__doc-name">${x => x.entry.name}</span>',
      '    </div>',
      '  `,',
      ');',
    ].join('\n');

    expect(await scopesAt(source, 'div class')).toContain('entity.name.tag.html');
    expect(await scopesAt(source, 'mc-lightbox__doc-name')).toContain('string.quoted.double.html');
    expect(await scopesAt(source, 'x.entry.name')).toContain('meta.embedded.line.ts');
  });

  it('opens a template whose type arguments are spread over several lines', async () => {
    // A `begin` pattern only ever sees one line, so the opener cannot be a
    // single regex that runs from `html` to the backtick.
    const source = [
      'export const toastItemTemplate: ViewTemplate<ToastItem, Toaster> = html<',
      '  ToastItem,',
      '  Toaster',
      '>`',
      '  <div class="toast" data-variant="${x => x.variant}">',
      '    <slot></slot>',
      '  </div>',
      '`;',
      'const after: number = 1;',
    ].join('\n');

    expect(await scopesAt(source, 'div class')).toContain('entity.name.tag.html');
    expect(await scopesAt(source, '"toast"')).toContain('string.quoted.double.html');
    expect(await scopesAt(source, 'slot>')).toContain('entity.name.tag.html');
    expect(await scopesAt(source, 'x.variant')).toContain('meta.embedded.line.ts');
    expect(await scopesAt(source, '  ToastItem,')).toContain('meta.type.parameters.ts');
    expect(await scopesAt(source, 'const after')).not.toContain('string.template.fast-element.ts');
  });

  it('opens a template whose type argument is itself generic', async () => {
    const source = 'const t = html<Row<Cell>>`<div class="a">hi</div>`;';
    expect(await scopesAt(source, 'div class')).toContain('entity.name.tag.html');
    expect(await scopesAt(source, 'Cell')).toContain('meta.type.parameters.ts');
  });

  it('opens a template whose backtick is on the line after the type arguments', async () => {
    const source = [
      'const t = html<',
      '  Foo',
      '>',
      '`<div class="a">hi</div>`;',
      'const after: number = 1;',
    ].join('\n');
    expect(await scopesAt(source, 'div class')).toContain('entity.name.tag.html');
    expect(await scopesAt(source, 'const after')).not.toContain('string.template.fast-element.ts');
  });

  it('gives up on a type argument list that no template follows', async () => {
    // Nothing writes this, but the rule that spans lines has to be unable to
    // swallow the rest of the file when the backtick it is waiting for never
    // arrives.
    const source = ['const t = html<Foo>;', 'const after: number = 1;'].join('\n');
    const scopes = await scopesAt(source, 'const after');
    expect(scopes).not.toContain('string.template.fast-element.ts');
    expect(scopes).not.toContain('meta.type.parameters.ts');
  });

  it('leaves a less-than comparison on `html` alone', async () => {
    const source = ['const flag = html < count;', 'const after: number = 1;'].join('\n');
    expect(await scopesAt(source, 'count')).not.toContain('meta.type.parameters.ts');
    expect(await scopesAt(source, 'const after')).not.toContain('meta.type.parameters.ts');
  });

  it('re-enters TypeScript inside ${…}', async () => {
    const source = 'const t = html`<div>${x => x.name}</div>`;';
    expect(await scopesAt(source, '=>')).toContain('meta.embedded.line.ts');
    expect(await scopesAt(source, '${')).toContain(
      'punctuation.definition.template-expression.begin.ts',
    );
  });

  it('scopes FAST binding prefixes', async () => {
    const source =
      'const t = html`<x-y :prop="${x => x.a}" ?hide="${x => x.b}" @click="${x => x.c}"></x-y>`;';
    for (const prefix of [':prop', '?hide', '@click']) {
      const scopes = await scopesAt(source, prefix);
      expect(scopes, prefix).toContain('keyword.operator.binding.fast-element');
      expect(scopes, prefix).toContain('punctuation.definition.binding.fast-element');
    }
    for (const name of ['prop', 'hide', 'click']) {
      expect(await scopesAt(source, name), name).toContain(
        'entity.other.attribute-name.binding.fast-element',
      );
    }
  });

  it('leaves nothing of a binding for the HTML grammar to call illegal', async () => {
    // text.html.basic has no rule for a `:`/`?`/`@` attribute name, so whatever
    // the binding rule does not consume lands on its illegal-character catch-all.
    const source =
      'const t = html`<x-y :prop="${x => x.a}" ?hide="${x => x.b}" @click="${x => x.c}"></x-y>`;';
    const illegal = (await tokenize(source))[0]
      .filter(t => t.scopes.some(s => s.startsWith('invalid.')))
      .map(t => source.slice(t.startIndex, t.endIndex));
    expect(illegal).toEqual([]);
  });

  it('accepts a single-quoted binding value', async () => {
    const source = "const t = html`<x-y @click='${x => x.go()}'></x-y>`;";
    expect(await scopesAt(source, '@click')).toContain('keyword.operator.binding.fast-element');
    expect(await scopesAt(source, 'x.go()')).toContain('meta.embedded.line.ts');
  });

  it('accepts an unquoted binding value', async () => {
    const source = 'const t = html`<x-y :prop=${x => x.a}></x-y>`;';
    expect(await scopesAt(source, ':prop')).toContain('keyword.operator.binding.fast-element');
    expect(await scopesAt(source, 'x.a')).toContain('meta.embedded.line.ts');
    const illegal = (await tokenize(source))[0].filter(t =>
      t.scopes.some(s => s.startsWith('invalid.')),
    );
    expect(illegal).toEqual([]);
  });

  it('re-enters TypeScript for a bare element expression such as ${ref(…)}', async () => {
    const source = "const t = html`<div ${ref('root')}></div>`;";
    expect(await scopesAt(source, "ref('root')")).toContain('meta.embedded.line.ts');
  });
});

describeWithCode('templates nested in an interpolation', () => {
  // `when(…, html`…`)` and `repeat(…, html`…`)` put a whole template inside a
  // ${…} of another one. Nothing about that is exotic, but it is the one shape
  // the opening grammar's `-string` selector shuts out, and by the time the
  // inner `html` is reached the stack is inside whatever anonymous rule
  // source.ts pushed for the enclosing argument list — so no pattern of ours
  // is on the stack to catch it either.
  const nested = [
    'export const template = html<Row>`',
    '  <ul>',
    '    ${when(',
    '      x => x.ready,',
    '      html<Row>`',
    '        <li class="row" @click="${x => x.pick()}">',
    '          <span>${x => x.name}</span>',
    '        </li>',
    '      `,',
    '    )}',
    '  </ul>',
    '`;',
    'const after: number = 1;',
  ].join('\n');

  it('hands the nested body to the HTML grammar', async () => {
    expect(await scopesAt(nested, 'li class')).toContain('entity.name.tag.html');
    expect(await scopesAt(nested, '"row"')).toContain('string.quoted.double.html');
    expect(await scopesAt(nested, 'span>')).toContain('entity.name.tag.html');
  });

  it('keeps bindings and ${…} alive inside the nested body', async () => {
    expect(await scopesAt(nested, '@click')).toContain('keyword.operator.binding.fast-element');
    expect(await scopesAt(nested, 'x.pick()')).toContain('meta.embedded.line.ts');
    expect(await scopesAt(nested, 'x.name')).toContain('meta.embedded.line.ts');
  });

  it('closes the nested template without closing the outer one', async () => {
    expect(await scopesAt(nested, '</ul>')).toContain('meta.embedded.block.html');
    expect(await scopesAt(nested, 'const after')).not.toContain('string.template.fast-element.ts');
  });

  it('nests a second time', async () => {
    const source = [
      'const t = html<A>`',
      '  ${repeat(',
      '    x => x.rows,',
      '    html<B>`',
      '      ${when(',
      '        x => x.open,',
      '        html<C>`<x-panel ?busy="${x => x.busy}">deep</x-panel>`,',
      '      )}',
      '    `,',
      '  )}',
      '`;',
    ].join('\n');
    expect(await scopesAt(source, 'x-panel')).toContain('entity.name.tag.html');
    expect(await scopesAt(source, '?busy')).toContain('keyword.operator.binding.fast-element');
    expect(await scopesAt(source, 'x.busy')).toContain('meta.embedded.line.ts');
  });

  it('nests a template whose type arguments are spread over several lines', async () => {
    const source = [
      'const t = html<A>`',
      '  ${when(',
      '    x => x.ready,',
      '    html<',
      '      Row,',
      '      Table',
      '    >`<li class="row">${x => x.name}</li>`,',
      '  )}',
      '`;',
      'const after: number = 1;',
    ].join('\n');
    expect(await scopesAt(source, 'li class')).toContain('entity.name.tag.html');
    expect(await scopesAt(source, '"row"')).toContain('string.quoted.double.html');
    expect(await scopesAt(source, 'x.name')).toContain('meta.embedded.line.ts');
    expect(await scopesAt(source, 'const after')).not.toContain('string.template.fast-element.ts');
  });

  it('nests inside a binding value', async () => {
    const source = 'const t = html`<x-y :template="${html`<b>deep</b>`}"></x-y>`;';
    expect(await scopesAt(source, 'b>deep')).toContain('entity.name.tag.html');
  });

  it('leaves a plain template literal in an interpolation to TypeScript', async () => {
    const source = 'const t = html`<div>${x => `plain ${x.n}`}</div>`;';
    const scopes = await scopesAt(source, 'plain ');
    expect(scopes).toContain('string.template.ts');
    // The outer template's own scope is on the stack throughout; what must not
    // be there is a second one, opened for a literal that is not ours.
    expect(scopes.filter(s => s === 'string.template.fast-element.ts')).toHaveLength(1);
  });

  it('leaves html`` inside a string in an interpolation alone', async () => {
    const source = 'const t = html`<div>${x => x.f("html`<i>no</i>`")}</div>`;';
    expect(await scopesAt(source, 'html`<i>')).toContain('string.quoted.double.ts');
    expect(await scopesAt(source, 'html`<i>')).not.toContain('entity.name.tag.html');
  });
});

describeWithCode('css`` templates', () => {
  it('hands the body to the CSS grammar', async () => {
    const scopes = await scopesAt('const s = css`:host { display: block; }`;', 'display');
    expect(scopes).toContain('meta.embedded.block.css');
    expect(scopes).toContain('support.type.property-name.css');
  });

  it('survives a multi-line template', async () => {
    const source = [
      'const styles = css`',
      '  :host {',
      '    display: flex;',
      '  }',
      '`;',
    ].join('\n');
    expect(await scopesAt(source, 'display')).toContain('support.type.property-name.css');
  });

  it('re-enters TypeScript inside ${…}', async () => {
    const source = 'const s = css`:host { color: ${theme.fg}; }`;';
    expect(await scopesAt(source, 'theme.fg')).toContain('meta.embedded.line.ts');
  });
});

describeWithCode('css.partial`` templates', () => {
  // A partial is a declaration list with no rule around it, so its body is
  // whatever goes *inside* a block, not a stylesheet.
  const partial = [
    'export const srOnlyStyle = css.partial`',
    '  position: absolute;',
    '  width: 1px;',
    '  overflow: hidden;',
    '  clip: rect(0, 0, 0, 0);',
    '  white-space: nowrap;',
    '`;',
    'const after: number = 1;',
  ].join('\n');

  it('hands a bare declaration list to the CSS grammar', async () => {
    expect(await scopesAt(partial, 'position')).toContain('meta.embedded.block.css');
    expect(await scopesAt(partial, 'position')).toContain('support.type.property-name.css');
    expect(await scopesAt(partial, 'absolute')).toContain('support.constant.property-value.css');
    expect(await scopesAt(partial, 'nowrap')).toContain('support.constant.property-value.css');
  });

  it('scopes a declaration exactly as a full stylesheet would', async () => {
    // The one scope a partial must *not* have is the block it has no braces
    // for; everything the CSS grammar says about the declaration itself has to
    // come out the same as it does inside a rule.
    const inRule = await scopesAt('const s = css`:host { clip: rect(0, 0, 0, 0); }`;', 'rect(');
    expect(inRule).toContain('meta.property-list.css');
    expect(await scopesAt(partial, 'rect(')).toEqual(
      inRule.filter(scope => scope !== 'meta.property-list.css'),
    );
    expect(await scopesAt(partial, '1px')).toContain('constant.numeric.css');
  });

  it('scopes the tag and returns to TypeScript after the closing backtick', async () => {
    expect(await scopesAt(partial, 'css.partial')).toContain(
      'entity.name.function.tagged-template.ts',
    );
    const scopes = await scopesAt(partial, 'const after');
    expect(scopes).not.toContain('meta.embedded.block.css');
    expect(scopes).not.toContain('string.template.fast-element.ts');
  });

  it('re-enters TypeScript inside ${…}', async () => {
    const source = 'const s = css.partial`color: ${theme.fg}; margin: 0;`;';
    expect(await scopesAt(source, 'theme.fg')).toContain('meta.embedded.line.ts');
    expect(await scopesAt(source, 'margin')).toContain('support.type.property-name.css');
  });

  it('still takes a partial that carries a whole rule', async () => {
    const source = ['const s = css.partial`', '  :host { display: flex; }', '`;'].join('\n');
    expect(await scopesAt(source, 'display')).toContain('support.type.property-name.css');
    expect(await scopesAt(source, 'flex')).toContain('support.constant.property-value.css');
  });

  it('leaves a partial of some other tag alone', async () => {
    const scopes = await scopesAt('const s = sql.partial`position: absolute;`;', 'position');
    expect(scopes).not.toContain('meta.embedded.block.css');
  });
});

describeWithCode('interpolations the outer grammars would otherwise swallow', () => {
  it('re-enters TypeScript inside an attribute value', async () => {
    const source = 'const t = html`<div class="${x => x.cls}"></div>`;';
    expect(await scopesAt(source, 'x.cls')).toContain('meta.embedded.line.ts');
  });

  it('re-enters TypeScript inside a bound attribute value', async () => {
    const source = 'const t = html`<x-y :entry="${x => x.entry}"></x-y>`;';
    expect(await scopesAt(source, 'x.entry}')).toContain('meta.embedded.line.ts');
  });

  it('re-enters TypeScript inside a CSS declaration block', async () => {
    const source = 'const s = css`:host { color: ${theme.fg}; }`;';
    expect(await scopesAt(source, 'theme.fg')).toContain('meta.embedded.line.ts');
  });

  it('does not re-inject inside an interpolation that is already TypeScript', async () => {
    // `?.` and `x ?y=` shapes inside an expression must stay TypeScript.
    const source = 'const t = html`<div>${x => (x.a ? x.b : x.c)}</div>`;';
    const scopes = await scopesAt(source, '?');
    expect(scopes).toContain('meta.embedded.line.ts');
    expect(scopes).not.toContain('keyword.operator.binding.fast-element');
  });
});

describeWithCode('where a template ends', () => {
  it('returns to TypeScript after the closing backtick', async () => {
    const source = [
      'const t = html<Foo>`',
      '  <div class="a">hi</div>',
      '`;',
      'const after: number = 1;',
    ].join('\n');
    const scopes = await scopesAt(source, 'const after');
    expect(scopes).not.toContain('meta.embedded.block.html');
    expect(scopes).not.toContain('string.template.fast-element.ts');
  });

  it('returns to TypeScript after a css template with a declaration block', async () => {
    const source = [
      'const s = css`',
      '  :host { display: flex; }',
      '`;',
      'const after: number = 1;',
    ].join('\n');
    const scopes = await scopesAt(source, 'const after');
    expect(scopes).not.toContain('meta.embedded.block.css');
    expect(scopes).not.toContain('string.template.fast-element.ts');
  });
});

describeWithCode('templates that are not ours', () => {
  it('leaves an unrelated tag alone', async () => {
    const scopes = await scopesAt('const t = gql`<div>hi</div>`;', 'div');
    expect(scopes).not.toContain('meta.embedded.block.html');
  });

  it('leaves html`` inside a comment alone', async () => {
    const scopes = await scopesAt('// const t = html`<div>hi</div>`;', 'div');
    expect(scopes).not.toContain('meta.embedded.block.html');
  });
});
