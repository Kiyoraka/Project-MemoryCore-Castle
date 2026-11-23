// The release's pure parts, shared by bump.mjs, latest-json.mjs and the release workflow:
// versions, the five places a version is written, the changelog and the updater's latest.json.

export const REPO = 'Kiyoraka/Project-MemoryCore-Castle'

const VERSION = /^(\d+)\.(\d+)\.(\d+)$/

export function isVersion(v) {
  return VERSION.test(String(v ?? ''))
}

// -1, 0 or 1 like a sort; versions are plain x.y.z.
export function compareVersions(a, b) {
  const pa = String(a).match(VERSION)
  const pb = String(b).match(VERSION)
  if (!pa || !pb) throw new Error(`Not a version: ${!pa ? a : b}`)
  for (let i = 1; i <= 3; i++) {
    const d = Number(pa[i]) - Number(pb[i])
    if (d) return Math.sign(d)
  }
  return 0
}

// The files that carry the version, relative to the repository root.
export const VERSION_FILES = ['package.json', 'package-lock.json', 'src-tauri/tauri.conf.json', 'src-tauri/Cargo.toml', 'src-tauri/Cargo.lock']

function replaceFirst(text, pattern, make, times = 1) {
  let left = times
  const out = text.replace(pattern, (...m) => (left-- > 0 ? make(...m) : m[0]))
  if (left > 0) throw new Error(`Expected ${times} version line(s), found ${times - left}`)
  return out
}

// The same text with the new version written in, by file; nothing else moves.
export function bumpText(file, text, version) {
  if (!isVersion(version)) throw new Error(`Not a version: ${version}`)
  const name = file.replace(/\\/g, '/').split('/').pop()
  const json = /"version": "[^"]*"/g
  const put = () => `"version": "${version}"`
  switch (name) {
    case 'package.json':
    case 'tauri.conf.json':
      return replaceFirst(text, json, put)
    case 'package-lock.json':
      // the lock names its own version twice: at the top and for the root package
      return replaceFirst(text, json, put, 2)
    case 'Cargo.toml':
      return replaceFirst(text, /^version = "[^"]*"/gm, () => `version = "${version}"`)
    case 'Cargo.lock':
      return replaceFirst(text, /(name = "memorycore-castle"\r?\nversion = )"[^"]*"/g, (_, head) => `${head}"${version}"`)
    default:
      throw new Error(`No version is kept in ${file}`)
  }
}

export function versionOf(packageJsonText) {
  return JSON.parse(packageJsonText).version
}

// The changelog's sections: [{ title, version, date, body }] in file order.
export function changelogSections(text) {
  const sections = []
  let current = null
  for (const line of String(text).split(/\r?\n/)) {
    const head = line.match(/^## (.+?)\s*$/)
    if (head) {
      const m = head[1].match(/^(\d+\.\d+\.\d+)(?:\s+-\s+(\d{4}-\d{2}-\d{2}))?$/)
      current = { title: head[1], version: m ? m[1] : null, date: m?.[2] ?? null, lines: [] }
      sections.push(current)
    } else if (current) current.lines.push(line)
  }
  return sections.map(({ lines, ...s }) => ({ ...s, body: lines.join('\n').trim() }))
}

// The words for one version ('' when it has none).
export function changelogSection(text, version) {
  return changelogSections(text).find((s) => s.version === version)?.body ?? ''
}

// Moves what waits under "## Unreleased" into the version's section (made if missing, right under
// Unreleased) and dates it; Unreleased stays, empty.
export function releaseChangelog(text, version, date) {
  if (!isVersion(version)) throw new Error(`Not a version: ${version}`)
  const lines = String(text).split(/\r?\n/)
  const start = lines.findIndex((l) => /^## Unreleased\s*$/.test(l))
  if (start < 0) throw new Error('The changelog has no "## Unreleased" heading.')
  let end = lines.findIndex((l, i) => i > start && l.startsWith('## '))
  if (end < 0) end = lines.length
  const waiting = lines.slice(start + 1, end).join('\n').trim()
  const rest = lines.slice(end)
  const at = rest.findIndex((l) => new RegExp(`^## ${version.replace(/\./g, '\\.')}(\\s|$)`).test(l))
  const heading = `## ${version} - ${date}`
  let out
  if (at >= 0) {
    let next = rest.findIndex((l, i) => i > at && l.startsWith('## '))
    if (next < 0) next = rest.length
    const had = rest.slice(at + 1, next).join('\n').trim()
    const body = [waiting, had].filter(Boolean).join('\n')
    out = [...lines.slice(0, start + 1), '', heading, '', body, '', ...rest.slice(next)]
  } else {
    if (!waiting) throw new Error(`Nothing waits under "## Unreleased" for ${version}.`)
    out = [...lines.slice(0, start + 1), '', heading, '', waiting, '', ...rest]
  }
  return out.join('\n').replace(/\n{3,}/g, '\n\n')
}

// ---- weighing a release before its version is chosen ----
// A release with new rooms, a new schema or many new features is a minor release, never "the next
// patch" by habit (0.2.28 shipped three new rooms as a patch and was withdrawn for 0.3.0, Nov 23 2026).
// Calibrated on Castle's own history: 0.2.16 to 0.2.27 each added at most one room and two migrations.
export const MINOR_WHEN = { rooms: 2, migrations: 5, features: 10 }

// 'major', 'minor' or 'patch': the step from one version to the next.
export function changeKind(current, next) {
  const [a, b] = [current, next].map((v) => {
    const m = String(v).match(VERSION)
    if (!m) throw new Error(`Not a version: ${v}`)
    return m.slice(1).map(Number)
  })
  if (b[0] !== a[0]) return 'major'
  if (b[1] !== a[1]) return 'minor'
  return 'patch'
}

const RANK = { patch: 0, minor: 1, major: 2 }

// The release's weight from what it adds since the last tag: the new room views, the new migration
// files and the changelog lines under "## Unreleased" that are not fixes. Never suggests a major
// release on its own: 1.0.0 is the owner's word.
export function measureRelease({ views = [], migrations = [], unreleased = '' } = {}) {
  const features = String(unreleased)
    .split(/\r?\n/)
    .filter((l) => /^\s*-\s+\S/.test(l) && !/^\s*-\s+Fixed\b/i.test(l)).length
  const counts = { rooms: views.length, migrations: migrations.length, features }
  const reasons = []
  if (counts.rooms >= MINOR_WHEN.rooms) reasons.push(`${counts.rooms} new rooms`)
  if (counts.migrations >= MINOR_WHEN.migrations) reasons.push(`${counts.migrations} new database migrations`)
  if (counts.features >= MINOR_WHEN.features) reasons.push(`${counts.features} new feature lines`)
  return { kind: reasons.length ? 'minor' : 'patch', reasons, counts }
}

// The next version of the given kind.
export function nextVersion(current, kind) {
  const m = String(current).match(VERSION)
  if (!m) throw new Error(`Not a version: ${current}`)
  const [x, y, z] = m.slice(1).map(Number)
  if (kind === 'major') return `${x + 1}.0.0`
  if (kind === 'minor') return `${x}.${y + 1}.0`
  return `${x}.${y}.${z + 1}`
}

// Whether a chosen version is smaller than the release weighs: a patch for a minor release is
// refused unless the owner said so on purpose (`--patch`).
export function versionTooSmall(current, chosen, measured) {
  return RANK[changeKind(current, chosen)] < RANK[measured.kind]
}

// The updater's latest.json (the shape tauri-plugin-updater reads).
export function latestJson({ version, notes, date, signature, url }) {
  if (!isVersion(version)) throw new Error(`Not a version: ${version}`)
  if (!signature || !url) throw new Error('latest.json needs the signature and the download address.')
  return {
    version,
    notes: notes ?? '',
    pub_date: new Date(date).toISOString().replace(/\.\d{3}Z$/, 'Z'),
    platforms: { 'windows-x86_64': { signature: signature.trim(), url } },
  }
}

// GitHub turns spaces in an asset's name into dots, so release files get a plain name first.
export function assetName(installerName) {
  return installerName.replace(/\s+/g, '-')
}

export function downloadUrl(version, asset, repo = REPO) {
  return `https://github.com/${repo}/releases/download/v${version}/${asset}`
}
