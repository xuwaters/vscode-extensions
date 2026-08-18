import * as path from 'path';
import { describe, expect, it } from 'vitest';
import { baseFor, expand } from './exportPath.js';

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

describe('the base an export writes to', () => {
  const dir = path.join(path.sep, 'proj', 'out');

  it('drops the extension the dialog added', () => {
    expect(baseFor(path.join(dir, 'paper.pdf'), 'pdf')).toBe(
      path.join(dir, 'paper'),
    );
  });

  it('drops it whatever case the file system handed back', () => {
    expect(baseFor(path.join(dir, 'PAPER.PDF'), 'pdf')).toBe(
      path.join(dir, 'PAPER'),
    );
  });

  it('leaves a name with no extension alone', () => {
    expect(baseFor(path.join(dir, 'paper'), 'pdf')).toBe(path.join(dir, 'paper'));
  });

  // A reader who typed `paper.v2` meant it; only the format's own extension is
  // the dialog's doing.
  it('keeps a suffix that is not the format', () => {
    expect(baseFor(path.join(dir, 'paper.v2'), 'pdf')).toBe(
      path.join(dir, 'paper.v2'),
    );
  });

  it('keeps the rest of a multi-part name', () => {
    expect(baseFor(path.join(dir, 'paper.v2.png'), 'png')).toBe(
      path.join(dir, 'paper.v2'),
    );
  });
});
