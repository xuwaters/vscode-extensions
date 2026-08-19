import { describe, expect, it } from 'vitest';
import { defaultProjectName, validateProjectName } from './projectName.js';

describe('the folder name a template suggests', () => {
  it('drops the namespace and the version', () => {
    expect(defaultProjectName('@preview/charged-ieee:0.1.4')).toBe('charged-ieee');
  });

  it('copes with a spec that carries no version', () => {
    expect(defaultProjectName('@preview/modern-cv')).toBe('modern-cv');
  });

  it('copes with a bare package name', () => {
    expect(defaultProjectName('touying:0.6.1')).toBe('touying');
  });

  it('ignores the whitespace an input box leaves behind', () => {
    expect(defaultProjectName('  @preview/basic-report:0.3.0  ')).toBe(
      'basic-report',
    );
  });
});

describe('validating a new project folder name', () => {
  it('accepts an ordinary name', () => {
    expect(validateProjectName('my-paper')).toBeUndefined();
  });

  it('accepts a name with spaces in the middle', () => {
    expect(validateProjectName('IEEE paper 2026')).toBeUndefined();
  });

  it('rejects nothing at all', () => {
    expect(validateProjectName('')).toBeDefined();
    expect(validateProjectName('   ')).toBeDefined();
  });

  it('rejects a path rather than creating the intermediate folders', () => {
    expect(validateProjectName('papers/ieee')).toBeDefined();
    expect(validateProjectName('papers\\ieee')).toBeDefined();
  });

  it('rejects the directory shorthands', () => {
    expect(validateProjectName('.')).toBeDefined();
    expect(validateProjectName('..')).toBeDefined();
  });

  it('rejects characters that will not survive a round trip to Windows', () => {
    expect(validateProjectName('paper:v2')).toBeDefined();
    expect(validateProjectName('what?')).toBeDefined();
    expect(validateProjectName('a|b')).toBeDefined();
  });

  it('rejects a trailing dot or space, which Windows would eat', () => {
    expect(validateProjectName('report.')).toBeDefined();
    // Trimmed first, so a name that is only spaced out is still fine.
    expect(validateProjectName(' report ')).toBeUndefined();
  });

  it('allows a dotted name that does not end in a dot', () => {
    expect(validateProjectName('paper.v2')).toBeUndefined();
  });
});
