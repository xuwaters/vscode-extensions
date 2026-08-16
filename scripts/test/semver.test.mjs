import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { bumpSemver, isSemver } from '../lib/semver.mjs';

describe('semver', () => {
  it('increments each part and resets the ones below it', () => {
    assert.equal(bumpSemver('1.2.3', 'patch'), '1.2.4');
    assert.equal(bumpSemver('1.2.3', 'minor'), '1.3.0');
    assert.equal(bumpSemver('1.2.3', 'major'), '2.0.0');
  });

  it('carries into wider numbers', () => {
    assert.equal(bumpSemver('0.9.9', 'patch'), '0.9.10');
    assert.equal(bumpSemver('1.99.0', 'minor'), '1.100.0');
  });

  it('drops a prerelease suffix', () => {
    assert.equal(bumpSemver('1.2.3-beta.1', 'patch'), '1.2.4');
  });

  it('refuses versions it cannot parse', () => {
    assert.throws(() => bumpSemver('1.2', 'patch'), /non-semver/);
    assert.throws(() => bumpSemver('v1.2.3', 'patch'), /non-semver/);
  });

  it('recognises valid versions', () => {
    assert.equal(isSemver('0.0.0'), true);
    assert.equal(isSemver('1.2.3-rc.1'), true);
    assert.equal(isSemver('1.2'), false);
    assert.equal(isSemver(''), false);
  });
});
