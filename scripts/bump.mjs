// node scripts/bump.mjs            weighs the release and suggests its version (writes nothing)
// node scripts/bump.mjs 0.3.0      writes the new version into every file that carries it and dates its
//                                  changelog section (what waits under "## Unreleased" moves into it)
// node scripts/bump.mjs 0.2.29 --patch   a patch on purpose when the release weighs as a minor one
// The release is weighed first (new rooms, migrations and feature lines since the last tag), and a version
// smaller than that weight is refused. Then commit, tag vX.Y.Z and push the tag: the release workflow
// builds, signs and publishes it.
import { execFileSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  VERSION_FILES,
  bumpText,
  changeKind,
  changelogSections,
  compareVersions,
  isVersion,
  measureRelease,
  nextVersion,
  releaseChangelog,
  versionOf,
  versionTooSmall,
} from './release-tools.mjs'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')
const args = process.argv.slice(2)
const version = args.find((a) => !a.startsWith('--'))
const patchOnPurpose = args.includes('--patch')
const current = versionOf(readFileSync(join(root, 'package.json'), 'utf8'))
const changelog = join(root, 'CHANGELOG.md')
const changelogText = readFileSync(changelog, 'utf8')

// ---- the weight of this release, from git and the changelog ----
function git(...a) {
  return execFileSync('git', a, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim()
}
function added(since, path) {
  return git('diff', '--name-only', '--diff-filter=A', since, '--', path).split('\n').filter(Boolean)
}
let measured = null
let since = ''
try {
  since = git('describe', '--tags', '--abbrev=0', '--match', 'v*')
  measured = measureRelease({
    views: added(since, 'src/views/*View.vue'),
    migrations: added(since, 'src-tauri/resources/schema/*.sql'),
    unreleased: changelogSections(changelogText).find((s) => s.title === 'Unreleased')?.body ?? '',
  })
} catch {
  console.warn('The release could not be weighed (no git or no earlier tag); choose the version with care.')
}
const weight = measured
  ? `Since ${since}: ${measured.counts.rooms} new room(s), ${measured.counts.migrations} migration(s), ${measured.counts.features} feature line(s)` +
    ` - a ${measured.kind} release${measured.reasons.length ? ` (${measured.reasons.join(', ')})` : ''}.`
  : ''

if (!version) {
  if (!measured) {
    console.error('Usage: node scripts/bump.mjs <x.y.z>')
    process.exit(1)
  }
  console.log(weight)
  console.log(`Suggested: ${nextVersion(current, measured.kind)}. Run: npm run bump ${nextVersion(current, measured.kind)}`)
  process.exit(1)
}
if (!isVersion(version)) {
  console.error('Usage: node scripts/bump.mjs [<x.y.z>] [--patch]')
  process.exit(1)
}
if (compareVersions(version, current) <= 0) {
  console.error(`${version} is not newer than ${current}.`)
  process.exit(1)
}
if (measured && versionTooSmall(current, version, measured) && !(patchOnPurpose && changeKind(current, version) === 'patch')) {
  console.error(weight)
  console.error(
    `${version} is a ${changeKind(current, version)} step, smaller than this release weighs. Use ${nextVersion(current, measured.kind)},` +
      ` or add --patch when a patch is meant on purpose.`,
  )
  process.exit(1)
}
if (weight) console.log(weight)

// everything is worked out first, then written, so a refusal changes nothing
const today = new Date()
const date = `${today.getFullYear()}-${String(today.getMonth() + 1).padStart(2, '0')}-${String(today.getDate()).padStart(2, '0')}`
const writes = VERSION_FILES.map((file) => {
  const path = join(root, file)
  return [path, bumpText(file, readFileSync(path, 'utf8'), version)]
})
writes.push([changelog, releaseChangelog(changelogText, version, date)])
for (const [path, text] of writes) writeFileSync(path, text)

// Settings > About's contributors wall is counted here, once a release (it never stops the bump)
const { refresh } = await import('./contributors.mjs')
console.log(await refresh().catch((why) => `Contributors: kept the last counts (${why.message || why})`))

console.log(`Castle ${current} -> ${version}. Next: commit, then "git tag v${version}" and push the tag.`)
