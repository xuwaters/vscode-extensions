// Just enough semver for extension versions, which are always plain `x.y.z`
// (VS Code rejects anything else on the Marketplace).

/** @typedef {'major' | 'minor' | 'patch'} ReleaseType */

const SEMVER = /^(\d+)\.(\d+)\.(\d+)(?:-[0-9A-Za-z.-]+)?$/;

/**
 * @param {string} version
 * @returns {boolean}
 */
export function isSemver(version) {
  return SEMVER.test(version);
}

/**
 * @param {string} version
 * @param {ReleaseType} release
 * @returns {string} The next version, with any prerelease suffix dropped.
 */
export function bumpSemver(version, release) {
  const match = SEMVER.exec(version);
  if (!match) throw new Error(`Cannot bump non-semver version '${version}'`);

  const [major, minor, patch] = [Number(match[1]), Number(match[2]), Number(match[3])];
  switch (release) {
    case 'major':
      return `${major + 1}.0.0`;
    case 'minor':
      return `${major}.${minor + 1}.0`;
    case 'patch':
      return `${major}.${minor}.${patch + 1}`;
    default:
      throw new Error(`Unknown release type '${release}'`);
  }
}
