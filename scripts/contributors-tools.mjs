// The contributors wall's pure parts (Settings > About), shared by contributors.mjs and its test:
// who has merged pull requests, and the file the page reads. The page itself never asks GitHub.

export const OWNER = 'Kiyoraka'

const isBot = (user) => !user || user.type === 'Bot' || /\[bot\]$/i.test(user.login || '')

// How many of a person's own pull request titles their card shows when no list is written by hand.
export const BUILT_KEPT = 6

// A pull request's title as a line on the wall: the `feat:` / `fix(scope):` tag and a trailing
// "(stacked on #83)" left out, the first letter made capital, kept short.
export function plainTitle(title = '') {
  let line = String(title)
    .trim()
    .replace(/^(feat|fix|docs|chore|refactor|perf|style|test|tests|build|ci|revert)(\([^)]*\))?!?\s*:\s*/i, '')
    .replace(/\s*\((stacked on|depends on)[^)]*\)\s*$/i, '')
    .trim()
  if (!line) return ''
  line = line[0].toUpperCase() + line.slice(1)
  return line.length > 90 ? `${line.slice(0, 89).trimEnd()}…` : line
}

// Merged pull requests per login, the owner and bots left out; the most first, then by login. Each
// person also carries what they built in their own words: their merged titles, newest first, so a new
// contributor's card says something real without anyone writing it by hand.
// `pulls` are GitHub's pull objects (state=closed); a closed pull that was never merged counts nothing.
export function countMerged(pulls, owner = OWNER) {
  const counts = new Map()
  const avatars = new Map()
  const titles = new Map()
  for (const pull of pulls || []) {
    const user = pull?.user
    if (!pull?.merged_at || isBot(user)) continue
    if (user.login.toLowerCase() === owner.toLowerCase()) continue
    counts.set(user.login, (counts.get(user.login) || 0) + 1)
    if (user.avatar_url) avatars.set(user.login, user.avatar_url)
    const line = plainTitle(pull.title)
    if (line) titles.set(user.login, [...(titles.get(user.login) || []), { at: pull.merged_at, line }])
  }
  const builtOf = (login) => {
    const seen = new Set()
    return (titles.get(login) || [])
      .slice()
      .sort((a, b) => String(b.at).localeCompare(String(a.at)))
      .map((t) => t.line)
      .filter((line) => !seen.has(line.toLowerCase()) && seen.add(line.toLowerCase()))
      .slice(0, BUILT_KEPT)
  }
  return [...counts.entries()]
    .map(([login, merged]) => ({ login, merged, avatar: avatars.get(login) || null, built: builtOf(login) }))
    .sort((a, b) => b.merged - a.merged || a.login.localeCompare(b.login))
}

// The file the page reads: when it was counted, the merged total and each person (with what their
// pull requests built, when they have titles).
export function countsFile(people, at) {
  return {
    at,
    merged: people.reduce((sum, p) => sum + p.merged, 0),
    people: people.map(({ login, merged, built }) => (built?.length ? { login, merged, built } : { login, merged })),
  }
}

// One line for the release road.
export function countsLine(file) {
  const n = file.people.length
  return `Contributors: ${n} ${n === 1 ? 'contributor' : 'contributors'}, ${file.merged} merged`
}

// A GitHub avatar at a small size (avatar urls may already carry a query).
export function smallAvatar(url, size = 96) {
  return `${url}${url.includes('?') ? '&' : '?'}s=${size}`
}
