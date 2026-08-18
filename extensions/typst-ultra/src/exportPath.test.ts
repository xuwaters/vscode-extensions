import * as path from 'path';
import { describe, expect, it } from 'vitest';
import { expand } from './exportPath.js';

describe('export.outputPath templating', () => {
  const root = path.join(path.sep, 'proj');
  const document = path.join(root, 'chapters', 'three.typ');

  it('expands $dir and $name to the document, by default', () => {
    expect(expand('$dir/$name', document, root)).toBe(
      path.join(root, 'chapters', 'three'),
    );
  });

  it('expands $root to the compile root', () => {
    expect(expand('$root/out/$name', document, root)).toBe(
      path.join(root, 'out', 'three'),
    );
  });

  it('treats a relative template as relative to the compile root', () => {
    expect(expand('build/$name', document, root)).toBe(
      path.join(root, 'build', 'three'),
    );
  });

  it('leaves an absolute template alone', () => {
    const absolute = path.join(path.sep, 'tmp', 'out', 'doc');
    expect(expand(absolute, document, root)).toBe(absolute);
  });

  it('strips only the typst extension from $name', () => {
    const dotted = path.join(root, 'my.paper.v2.typ');
    expect(expand('$dir/$name', dotted, root)).toBe(path.join(root, 'my.paper.v2'));
  });

  it('expands every occurrence of a placeholder', () => {
    expect(expand('$root/$name/$name', document, root)).toBe(
      path.join(root, 'three', 'three'),
    );
  });

  it('leaves an unknown placeholder alone rather than eating it', () => {
    expect(expand('$dir/$unknown', document, root)).toBe(
      path.join(root, 'chapters', '$unknown'),
    );
  });
});
