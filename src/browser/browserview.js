// The BROWSER window's toolbar page: what its new-tab page offers and what its address bar shows.

export const ARTIFACTS = 'https://claude.ai/code/artifacts'
// servers that answer a database client, not a web page
const NOT_PAGES = new Set(['mysqld', 'postgres', 'redis-server', 'mongod'])

const folderName = (folder) => String(folder ?? '').split(/[\\/]/).filter(Boolean).pop() ?? ''

/** The new tab's shortcuts: Artifacts, then each live web server once ('blank': none). */
export function newTabShortcuts(servers, setting) {
  if (setting === 'blank') return []
  const out = [{ name: 'Artifacts', url: ARTIFACTS, letter: 'A' }]
  const seen = new Set()
  for (const server of servers ?? []) {
    if (!server?.port || seen.has(server.port) || NOT_PAGES.has(server.program)) continue
    seen.add(server.port)
    const name = `${folderName(server.folder) || 'Server'} :${server.port}`
    out.push({ name, url: `http://localhost:${server.port}`, letter: name[0].toUpperCase() })
  }
  return out
}

/** The site check's line for the tab in front (Rust: site_rules::Warning), or '' when nothing is wrong. */
export function siteWarning(warning) {
  if (!warning?.verdict || warning.verdict === 'safe') return ''
  const word = warning.verdict === 'dangerous' ? 'dangerous' : 'suspicious'
  return `The Sentinel marked this site ${word}: ${warning.reason}`
}

/** The address of the tab in front, or '' for a new tab. */
export function shownAddress(tabs) {
  const tab = tabs?.list?.find((t) => t.id === tabs.active)
  return tab?.url ?? ''
}

// Sign-in pages where a Google sign-in fails or is not kept in an embedded window (F11).
const SIGN_IN = [
  (u) => u.hostname === 'claude.ai' && u.pathname.startsWith('/login'),
  (u) => u.hostname === 'accounts.google.com',
]

/** A line for the toolbar while a sign-in page is in front, else ''. */
export function signInHint(url) {
  let parsed
  try {
    parsed = new URL(url)
  } catch {
    return ''
  }
  if (parsed.protocol !== 'https:' || !SIGN_IN.some((match) => match(parsed))) return ''
  return 'Sign in with your email code: Google sign-in is not kept in this window'
}

/** The new tab's own lists as shortcut tiles: the most visited sites, then the newest bookmarks. */
export function libraryTiles(lists) {
  const visited = (lists?.mostVisited ?? []).map((v) => ({ name: v.host, url: v.url, letter: (v.host[0] ?? '?').toUpperCase() }))
  const marks = (lists?.bookmarks ?? []).map((b) => {
    const name = b.title || hostOf(b.url) || b.url
    return { name, url: b.url, letter: (name[0] ?? '?').toUpperCase() }
  })
  return { visited, marks }
}

function hostOf(url) {
  try {
    return new URL(url).host
  } catch {
    return ''
  }
}

/** The Fill button's words: one saved login names its user; several open a choice. */
export function fillLabel(logins) {
  if (!logins?.length) return ''
  if (logins.length === 1) return logins[0].username ? `Fill as ${logins[0].username}` : 'Fill'
  return `Fill (${logins.length} saved)`
}

// The DANGER screen (browser_guard.rs holds the page back): the threat in one line, and the guards.
const THREATS = {
  phishing: 'Phishing - this site copies a real sign-in page.',
  malware: 'Malware - this site may put harmful software on your computer.',
  unwanted: "Unwanted software - this site pushes programs you didn't ask for.",
  harmful: 'Harmful apps - this site offers apps that can hurt your device.',
  dangerous: 'Dangerous site - your companion marked it.',
}

/** The DANGER screen's words for a held-back page: its threat line, its reason, and every guard as
 * a chip, lit when it caught the page. `mac`: the platform's own guard is Safari's warning. */
export function dangerWords(danger, mac = false) {
  if (!danger) return null
  const caught = new Set(danger.caughtBy ?? [])
  const guards = [
    { id: 'sentinel', name: 'Sentinel' },
    { id: 'smartscreen', name: mac ? "Safari's warning" : 'SmartScreen' },
    { id: 'google', name: 'Google Safe Browsing' },
    { id: 'lookalike', name: 'Look-alike' },
  ]
  return {
    threat: THREATS[danger.threat] ?? THREATS.dangerous,
    reason: danger.reason ?? '',
    address: danger.url ?? '',
    chips: guards.map((g) => ({ ...g, lit: caught.has(g.id) })),
  }
}
