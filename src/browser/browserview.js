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

/** A tab chip's count of news, '' when none, capped at 99+. */
export function chipCount(unread) {
  const n = Number(unread) || 0
  if (n <= 0) return ''
  return n > 99 ? '99+' : String(n)
}

// The address bar's suggestions (browser_suggest.rs): past searches as pills on top, sites and
// bookmarks as cards below, one selection moving through both in that order.
/** The rows split as they are shown: { pills, cards }, each row keeping its place in the order. */
export function suggestionGroups(rows) {
  const list = rows ?? []
  const pills = list.filter((r) => r.kind === 'search')
  const cards = list.filter((r) => r.kind !== 'search')
  return { pills, cards, order: [...pills, ...cards] }
}

/** The selection after a key: -1 is the typed text itself; arrows wrap through the rows. */
export function moveSelection(index, key, count) {
  if (!count) return -1
  if (key === 'ArrowDown') return index + 1 >= count ? -1 : index + 1
  if (key === 'ArrowUp') return index <= -1 ? count - 1 : index - 1
  return index
}

/** What pressing Enter on a row types into the address bar: a past search its words (kept again),
 * a site or bookmark its address. */
export function suggestionInput(row) {
  if (!row) return ''
  return row.kind === 'search' ? row.text : row.url
}

// Pages side by side (browser_tabs.rs): the group shows as one stacked chip in the strip.
/** The group of pages side by side, or null: its tabs in column order, the chip's words and count. */
export function groupOf(tabs) {
  const ids = tabs?.panes ?? []
  if (ids.length < 2) return null
  const members = ids.map((id) => tabs.list.find((t) => t.id === id)).filter(Boolean)
  if (members.length < 2) return null
  const first = members[0].title || 'Page'
  return {
    ids,
    members,
    label: `${first} +${members.length - 1}`,
    unread: members.reduce((sum, t) => sum + (Number(t.unread) || 0), 0),
    list: members.map((t) => t.title).join('\n'),
    on: ids.includes(tabs.active),
  }
}

/** The single chips: every tab that is not in the group. */
export function singleTabs(tabs) {
  const inGroup = new Set((tabs?.panes ?? []).length >= 2 ? tabs.panes : [])
  return (tabs?.list ?? []).filter((t) => !inGroup.has(t.id))
}

/** The tabs on screen, left to right: the group when the tab in front is in it, else that tab. */
export function onScreen(tabs) {
  const active = tabs?.active
  if (active == null) return []
  const panes = tabs.panes ?? []
  return panes.length >= 2 && panes.includes(active) ? panes : [active]
}

/** New column shares after dragging the grip after column `at` by `dx` pixels across `width`:
 * the two columns beside it share the move, neither below a tenth. */
export function dragWidths(widths, at, dx, width, least = 0.1) {
  const next = [...widths]
  if (at < 0 || at >= next.length - 1 || !width) return next
  const pair = next[at] + next[at + 1]
  const left = Math.min(Math.max(next[at] + dx / width, least), pair - least)
  next[at] = left
  next[at + 1] = pair - left
  return next
}

// The translate row (browser_translate.rs): the page's language to the member's, Always, Original
// and Read bubbles.
/** A language's name from the list ([code, name] pairs), by its code or the page's own tag. */
export function languageLabel(code, languages) {
  const c = String(code ?? '').toLowerCase()
  if (!c) return 'This page'
  const list = languages ?? []
  const exact = list.find(([k]) => k.toLowerCase() === c)
  if (exact) return exact[1]
  const base = c.split(/[-_]/)[0]
  const near = base === 'zh' ? list.find(([k]) => k.toLowerCase().startsWith(c.slice(0, 5))) : list.find(([k]) => k.toLowerCase().split('-')[0] === base)
  return near ? near[1] : c.toUpperCase()
}

/** Whether two language tags are the same language (Chinese keeps its script). */
export function sameLanguage(page, mine) {
  const p = String(page ?? '').toLowerCase()
  const m = String(mine ?? '').toLowerCase()
  if (!p || !m) return false
  if (m.startsWith('zh')) return p === m || p.startsWith(m)
  return p.split(/[-_]/)[0] === m.split(/[-_]/)[0]
}

/** Whether the translate row shows for a tab: asked for, already translating, or a page that says
 * it is in another language than the member's. */
export function showTranslateRow(tab, mine, asked = false) {
  if (!tab?.url || tab.danger) return false
  if (asked || tab.translated) return true
  return Boolean(tab.lang) && !sameLanguage(tab.lang, mine)
}

/** The site a page is on, for Always. */
export function siteOf(url) {
  try {
    return new URL(url).hostname.toLowerCase()
  } catch {
    return ''
  }
}


// The Keep's look: chips, columns and the boxes the pages go into.
/** A chip's title, cut to 28 characters as the Keep's. */
export function chipTitle(title) {
  const t = String(title ?? '')
  return t.length > 28 ? `${t.slice(0, 27)}…` : t
}

/** The panes' grid columns: one share each, a 10 px grip between two (one column: the whole width). */
export function columnsOf(widths) {
  const list = widths?.length ? widths : [1]
  return list.map((w) => `${Math.max(Number(w) || 0, 0.01)}fr`).join(' 10px ')
}

/** A page box as the core takes it: whole pixels, or null while it has no real size. */
export function boundsOf(rect) {
  if (!rect) return null
  const box = { x: Math.round(rect.left), y: Math.round(rect.top), w: Math.round(rect.width), h: Math.round(rect.height) }
  return box.w >= 2 && box.h >= 2 ? box : null
}
