import { describe, expect, it } from 'vitest';
import { PageMemory } from './pageMemory.js';

describe('where each document was read to', () => {
  const paper = 'file:///proj/paper.typ';
  const notes = 'file:///proj/notes.typ';

  it('remembers a page per document', () => {
    const memory = new PageMemory();
    memory.park(paper, 12);
    memory.park(notes, 3);

    expect(memory.peek(paper)).toBe(12);
    expect(memory.peek(notes)).toBe(3);
  });

  it('has nothing to say about a document it has not seen', () => {
    expect(new PageMemory().peek(paper)).toBeUndefined();
  });

  it('leaves the record in place for peek and clears it for take', () => {
    const memory = new PageMemory();
    memory.park(paper, 7);

    expect(memory.peek(paper)).toBe(7);
    expect(memory.take(paper)).toBe(7);
    expect(memory.peek(paper)).toBeUndefined();
  });

  it('forgets on request', () => {
    const memory = new PageMemory();
    memory.park(paper, 4);
    memory.forget(paper);

    expect(memory.peek(paper)).toBeUndefined();
  });

  // A page index arrives from the webview, which is an input boundary even
  // though we wrote both sides of it.
  it('refuses a page index that is not one', () => {
    const memory = new PageMemory();
    for (const page of [-1, 1.5, Number.NaN]) {
      memory.park(paper, page);
    }
    expect(memory.peek(paper)).toBeUndefined();
  });
});
