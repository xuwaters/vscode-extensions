import { describe, expect, it } from 'vitest';
import type { ViewerPlace } from './messages.js';
import { PLACES_LIMIT, withPlace, type StoredPlaces } from './placeMemory.js';

const place = (page: number): ViewerPlace => ({
  page,
  zoom: 1,
  fit: 'fit-width',
  rotation: 0,
  inverted: false,
  outlineVisible: false,
  outlineWidth: 240,
  offsetRatio: 0,
});

describe('remembering where a document was read to', () => {
  it('records a place', () => {
    const next = withPlace({}, 'a.pdf', place(7), 100);
    expect(next['a.pdf']).toEqual({ place: place(7), at: 100 });
  });

  it('overwrites an earlier reading of the same document', () => {
    const first = withPlace({}, 'a.pdf', place(7), 100);
    const second = withPlace(first, 'a.pdf', place(9), 200);
    expect(Object.keys(second)).toEqual(['a.pdf']);
    expect(second['a.pdf']!.place.page).toBe(9);
  });

  it('does not mutate what it was given', () => {
    const before: StoredPlaces = {};
    withPlace(before, 'a.pdf', place(1), 1);
    expect(before).toEqual({});
  });

  it('drops the least recently read once past the limit', () => {
    let store: StoredPlaces = {};
    for (let i = 0; i < PLACES_LIMIT; i += 1) {
      store = withPlace(store, `doc${i}.pdf`, place(1), i);
    }
    store = withPlace(store, 'new.pdf', place(1), 10_000);

    expect(Object.keys(store)).toHaveLength(PLACES_LIMIT);
    expect(store['doc0.pdf']).toBeUndefined();
    expect(store['new.pdf']).toBeDefined();
    expect(store[`doc${PLACES_LIMIT - 1}.pdf`]).toBeDefined();
  });

  it('keeps the entry just written even when it is the oldest by clock', () => {
    // A clock that went backwards must not evict the document being read.
    let store: StoredPlaces = {};
    for (let i = 0; i < PLACES_LIMIT; i += 1) {
      store = withPlace(store, `doc${i}.pdf`, place(1), 1000 + i);
    }
    store = withPlace(store, 'new.pdf', place(1), 0);
    expect(store['new.pdf']).toBeDefined();
    expect(Object.keys(store)).toHaveLength(PLACES_LIMIT);
  });
});
