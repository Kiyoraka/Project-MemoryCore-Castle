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
/** The Passwords sheet's search: the site or the username, case aside. */
export function filterLogins(logins, search) {
  const q = String(search ?? '').trim().toLowerCase()
  if (!q) return logins ?? []
  return (logins ?? []).filter((l) => `${l.host} ${l.username ?? ''}`.toLowerCase().includes(q))
}

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

/** The bookmarks bar from every bookmark (folders like "Bookmarks bar/Work"): bookmarks right on the
 * bar are site pills; the rest gather under their first folder (the bar's own sub-folders by their own
 * name), each keeping the rest of its folder path as `sub`. */
export function barOf(marks) {
  const BAR = 'bookmarks bar'
  const sites = []
  const folders = []
  for (const m of marks ?? []) {
    const parts = String(m.folder ?? '').split('/').map((p) => p.trim()).filter(Boolean)
    const onBar = parts.length === 0 || (parts.length === 1 && parts[0].toLowerCase() === BAR)
    const item = { title: m.title || m.url, url: m.url }
    if (onBar) {
      sites.push(item)
      continue
    }
    const underBar = parts[0].toLowerCase() === BAR
    const name = underBar ? parts[1] : parts[0]
    const sub = parts.slice(underBar ? 2 : 1).join(' / ')
    let folder = folders.find((f) => f.name === name)
    if (!folder) folders.push((folder = { name, items: [] }))
    folder.items.push({ ...item, sub })
  }
  return { folders, sites }
}

// ---------- the downloader (ported from the Keep, browser-downloads-2) ----------

/** Bytes in plain words: 512 B, 1.4 KB, 12.4 MB, 1.8 GB. */
export function bytesWords(n) {
  const v = Number(n) || 0
  if (v < 1024) return `${v} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  let x = v / 1024
  let i = 0
  while (x >= 1024 && i < units.length - 1) {
    x /= 1024
    i++
  }
  return `${x >= 100 ? Math.round(x) : x.toFixed(1)} ${units[i]}`
}

/** Time left in plain words, or '' when it cannot be known. */
export function timeLeft(done, size, speed) {
  if (!size || !speed || done >= size) return ''
  const s = Math.ceil((size - done) / speed)
  if (s < 60) return `${s} s left`
  if (s < 3600) return `${Math.ceil(s / 60)} min left`
  return `${Math.floor(s / 3600)} h ${Math.ceil((s % 3600) / 60)} min left`
}

const DL_COLOURS = { running: 'mint', queued: 'mint', native: 'mint', paused: 'amber', done: 'gold', failed: 'coral' }
const DL_ICONS = { Video: 'film', Music: 'note', Documents: 'doc', Programs: 'box', Compressed: 'box', Other: 'file' }

/** One download's card: its icon, bar colour, percent and the small line under the bar. */
export function dlCard(item) {
  const size = item?.size ?? 0
  const done = item?.done ?? 0
  const percent = item?.state === 'done' ? 100 : size ? Math.min(100, Math.floor((done / size) * 100)) : 0
  let line
  if (item?.state === 'running') line = [item.parts > 1 ? `${item.parts} parts` : '', item.speed ? `${bytesWords(item.speed)}/s` : '', timeLeft(done, size, item.speed)].filter(Boolean).join(' · ') || 'Starting…'
  else if (item?.state === 'queued') line = 'Queued'
  else if (item?.state === 'paused') line = `Paused at ${percent}%`
  else if (item?.state === 'done') line = size ? bytesWords(size) : 'Done'
  else if (item?.state === 'failed') line = item.line || 'It did not finish.'
  else line = item?.line || ''
  if (item?.line && ['running', 'paused'].includes(item?.state)) line = `${line} · ${item.line}`
  return { icon: DL_ICONS[item?.category] ?? 'file', colour: DL_COLOURS[item?.state] ?? 'mint', percent, line }
}

/** The downloads under one category pill ('All' is everything). */
export const dlFilter = (items, category) => (items ?? []).filter((i) => category === 'All' || i.category === category)

/** A link the video tool reads (YouTube, Threads and the like), not a plain file. */
export function isVideoLink(url) {
  try {
    const host = new URL(url).hostname.replace(/^www\./, '').replace(/^m\./, '')
    return ['youtube.com', 'youtu.be', 'threads.net', 'threads.com', 'instagram.com', 'tiktok.com', 'facebook.com', 'fb.watch', 'x.com', 'twitter.com', 'vimeo.com', 'dailymotion.com'].some((h) => host === h || host.endsWith(`.${h}`))
  } catch {
    return false
  }
}

/** The chip's ring: how far all running downloads have come together (null when none run). */
export function dlRing(items) {
  const running = (items ?? []).filter((i) => i.state === 'running' && i.size)
  if (!running.length) return (items ?? []).some((i) => ['running', 'native'].includes(i.state)) ? 0 : null
  const size = running.reduce((n, i) => n + i.size, 0)
  return Math.floor((running.reduce((n, i) => n + i.done, 0) / size) * 100)
}

/** A page box beside an open side panel (Passwords, Downloads): it ends 8 px before the panel begins,
 * so the page stays in view and never covers the panel; null when nothing is left of it. */
export function besideSheet(box, sheet) {
  if (!box || !sheet) return box
  const right = Math.round(sheet.left) - 8
  if (box.x + box.w <= right) return box
  const w = right - box.x
  return w >= 2 ? { ...box, w } : null
}

// ---------- a tab in a window of its own (browser_out.rs): the drag and the out chips ----------

/** The browser's home window, as a tab names it; any other window is one of its own. */
export const HOME = 'home'
/** How far a chip must move before a press becomes a drag (a smaller move is still a click). */
export const DRAG_PX = 8

/** Whether a tab lives in the home (a tab that names no window does). */
export const atHome = (tab) => !tab?.window || tab.window === HOME

/** The tabs as the home shows them: only its own tabs, its group and the tab in front among them. */
export function homeView(tabs) {
  const list = (tabs?.list ?? []).filter(atHome)
  const ids = new Set(list.map((t) => t.id))
  const panes = (tabs?.panes ?? []).filter((id) => ids.has(id))
  return { ...tabs, list, panes: panes.length >= 2 ? panes : [], active: ids.has(tabs?.active) ? tabs.active : null }
}

/** Whether the pointer has moved far enough from where it went down to lift the chip. */
export function dragPast(start, now, px = DRAG_PX) {
  if (!start || !now) return false
  return Math.hypot((now.x ?? 0) - (start.x ?? 0), (now.y ?? 0) - (start.y ?? 0)) > px
}

/** The room's edge to glow while a chip is dragged ('left', 'right', 'top' or 'bottom'): past an edge,
 * the one the pointer went furthest past; inside, the nearest one once the pointer is within `reach`
 * pixels of it; '' otherwise. `rect` is the room's box ({ left, top, right, bottom }). */
export function nearEdge(rect, point, reach = 48) {
  if (!rect || !point) return ''
  const gaps = [
    ['left', point.x - rect.left],
    ['right', rect.right - point.x],
    ['top', point.y - rect.top],
    ['bottom', rect.bottom - point.y],
  ]
  const past = gaps.filter(([, d]) => d < 0).sort((a, b) => a[1] - b[1])
  if (past.length) return past[0][0]
  const [side, gap] = [...gaps].sort((a, b) => a[1] - b[1])[0]
  return gap <= reach ? side : ''
}

/** The home's chips for its tabs that are out: one per window of its own (pages that went out together
 * are one chip), with the first page's name and how many more, their news together, private when any
 * page is, held back when any is under the DANGER screen. Pressing one brings its window forward. */
export function outChips(list) {
  const byWindow = new Map()
  for (const t of list ?? []) {
    if (atHome(t)) continue
    if (!byWindow.has(t.window)) byWindow.set(t.window, [])
    byWindow.get(t.window).push(t)
  }
  return [...byWindow.entries()].map(([window, members]) => {
    const first = members[0].title || 'Page'
    return {
      window,
      ids: members.map((t) => t.id),
      title: members.length > 1 ? `${first} +${members.length - 1}` : first,
      list: members.map((t) => t.title).join('\n'),
      unread: members.reduce((n, t) => n + (Number(t.unread) || 0), 0),
      private: members.some((t) => !!t.private),
      danger: members.some((t) => !!t.danger),
    }
  })
}

/** What the drag's card says under itself for where a drop would go now; nothing while it would stay. */
export function ghostLine(state) {
  if (state === 'out') return 'Let go to open in a new window'
  if (state === 'in') return 'Let go to bring it back'
  return ''
}
