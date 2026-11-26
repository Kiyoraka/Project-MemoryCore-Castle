// The room launcher (APPS): which rooms sit in which group, which are pinned to the top bar, and what the
// top bar holds. Pure, so it can be tested. The launcher's own id is `launcher`; the room that used to be
// called APPS (the Start-menu apps) is `apps`, labelled DESKTOP.

// HOME and the launcher are always in the top bar and are never pinned.
export const FIXED = ['home', 'launcher']
// The rooms pinned until the member chooses their own (the Rust default is the same list).
export const DEFAULT_PINS = ['chat', 'projects', 'dev']
export const MAX_PINS = 16

// Tooltips on the tiles learn from a repeated action. The first two tips each wait a moment (someone may
// only want to know about one room); from the third tile in a row the tips show at once, until the pointer
// has been off the tiles for a short while. These are the waits, in milliseconds, and the count.
export const TIP_FIRST_MS = 450
export const TIP_LEAVE_MS = 400
export const TIP_INSTANT_AFTER = 2
export function tipDelay(shownInARow) {
  return shownInARow >= TIP_INSTANT_AFTER ? 0 : TIP_FIRST_MS
}

export const LAUNCHER_TAB = { id: 'launcher', label: 'APPS', fixed: true, title: 'All your rooms' }

// Quiet group labels; group order and membership live here and nowhere else.
export const GROUPS = [
  { id: 'companion', label: 'Companion', rooms: ['home', 'chat', 'myprofile', 'diary', 'appearance', 'finance'] },
  { id: 'work', label: 'Work', rooms: ['projects', 'dev', 'scriptorium', 'automations', 'calendar', 'browser', 'artifacts'] },
  { id: 'create', label: 'Create', rooms: ['studio', 'photo', 'story', 'world', 'library', 'worldsmith'] },
  { id: 'connect', label: 'Connect', rooms: ['messaging', 'market'] },
  { id: 'system', label: 'System', rooms: ['theme', 'apps'] },
]
const OTHER = { id: 'more', label: 'More' }

// One faint line each, shown on hover and focus.
export const HINTS = {
  home: 'Your companion, the chat and your cards',
  chat: 'The chat room with the Foundation',
  myprofile: 'A reading of you, from your diary',
  diary: "Your companion's diary",
  appearance: "Your companion's look and wardrobe",
  projects: 'Your projects and tasks',
  dev: 'Terminals for your projects',
  scriptorium: 'Your own documents, sheets, slides and PDFs',
  automations: 'Jobs that run on a schedule or a trigger',
  calendar: 'Everything with a date, and your events',
  finance: 'Receipts your companion reads, kept on this computer',
  studio: 'Pictures, songs, video and voice',
  photo: 'A basic photo editor with your companion',
  story: 'A story told with your companion',
  world: 'Worlds and screens',
  library: 'Books for you and your companion',
  worldsmith: 'Your own offline single-player games',
  messaging: 'Your companion on Telegram, Discord and WhatsApp',
  market: 'Prices and a pretend shop',
  theme: 'How Castle looks',
  apps: 'Your Start-menu apps',
  browser: 'Web pages beside Castle, on the right half of the screen',
  artifacts: 'Your Claude artifacts: pages, decks and designs',
}

// Tiles that open the BROWSER window (CASTLE-12/13) instead of a room: url null is a new tab.
const WINDOW_ROOMS = {
  browser: { url: null },
  artifacts: { url: 'https://claude.ai/code/artifacts' },
}
/** { url } for a tile that opens the browser window, or null for a room. `opens` is Settings'
 * Browser opens: inside Castle ('castle') BROWSER is a room of its own, and ARTIFACTS still opens the
 * browser (which then turns Castle to BROWSER). */
export function windowRoom(id, opens = 'window') {
  if (opens === 'castle' && id === 'browser') return null
  return Object.hasOwn(WINDOW_ROOMS, id ?? '') ? { ...WINDOW_ROOMS[id] } : null
}

/** The rooms (the visible tabs) in their groups. A room appears exactly once; one this list does not know
 * lands in a last group, so a room added later is never lost. Empty groups are left out. */
export function groupRooms(tabs) {
  const byId = new Map((tabs ?? []).map((t) => [t.id, t]))
  const used = new Set()
  const groups = GROUPS.map((g) => ({
    id: g.id,
    label: g.label,
    tabs: g.rooms.filter((id) => byId.has(id) && !used.has(id)).map((id) => (used.add(id), byId.get(id))),
  }))
  const loose = (tabs ?? []).filter((t) => !used.has(t.id))
  if (loose.length) groups.push({ ...OTHER, tabs: loose })
  return groups.filter((g) => g.tabs.length)
}

/** The pins as they should be saved: plain unique ids of rooms that exist, never HOME or the launcher. */
export function cleanPins(ids, known = []) {
  const real = new Set(known)
  const seen = new Set()
  return (ids ?? [])
    .map((id) => String(id).trim())
    .filter((id) => id && !FIXED.includes(id) && (real.size === 0 || real.has(id)) && !seen.has(id) && seen.add(id))
    .slice(0, MAX_PINS)
}

/** The rows of Settings > Rooms > Top bar: every room the launcher shows except HOME and APPS, in the
 * launcher's own order, so the list matches the tiles. `labels` maps a room id to its name; a room
 * with no name there is left out. */
export function pinRows(labels = {}) {
  return GROUPS.flatMap((g) => g.rooms)
    .filter((id) => !FIXED.includes(id) && labels[id])
    .map((id) => ({ id, label: labels[id] }))
}

/** The pins with one room pinned or unpinned. HOME and the launcher cannot be pinned. */
export function setPinned(pins, id, on, known = []) {
  if (FIXED.includes(id)) return cleanPins(pins, known)
  const without = (pins ?? []).filter((p) => p !== id)
  return cleanPins(on ? [...without, id] : without, known)
}

/** What the top bar holds: HOME, the launcher, the pinned rooms in the member's tab order, and the room that
 * is open if it is not pinned (so the bar always shows where you are). `tabs` are the visible rooms in tab order. */
export function headerTabs(tabs, pins, active) {
  const home = (tabs ?? []).find((t) => t.id === 'home') ?? { id: 'home', label: 'HOME' }
  const rest = (tabs ?? []).filter((t) => t.id !== 'home' && (pins.includes(t.id) || t.id === active))
  return [{ ...home, fixed: true, title: 'Your companion, the chat and your cards' }, LAUNCHER_TAB, ...rest]
}

/** The first letter shown on a tile that has no icon yet. */
export function initialOf(label) {
  return (String(label ?? '').trim()[0] ?? '?').toUpperCase()
}

// ---- phase 2: search, keyboard and badges ----

/**
 * The rooms whose name or hint holds every word typed (case does not matter), best first: a room whose
 * NAME matches comes before one only its hint matches (typing "chat" finds CHAT before HOME, whose hint
 * mentions the chat), and a name that starts with the word comes first of all. Otherwise the order is kept.
 */
export function filterTabs(tabs, query) {
  const words = String(query ?? '').toLowerCase().split(/\s+/).filter(Boolean)
  if (!words.length) return tabs ?? []
  const scored = []
  for (const t of tabs ?? []) {
    const name = `${t.label} ${t.id}`.toLowerCase()
    const hay = `${name} ${HINTS[t.id] ?? ''}`.toLowerCase()
    if (!words.every((w) => hay.includes(w))) continue
    const named = words.every((w) => name.includes(w))
    const rank = named ? (t.label.toLowerCase().startsWith(words[0]) ? 0 : 1) : 2
    scored.push({ t, rank, at: scored.length })
  }
  return scored.sort((a, b) => a.rank - b.rank || a.at - b.at).map((s) => s.t)
}

/**
 * What the launcher shows for a search. Nothing typed: the groups. Something typed: one list of the
 * matches, best first, so the tile at the top is the one Enter opens.
 */
export function filterGroups(groups, query) {
  if (!String(query ?? '').trim()) return groups
  const tabs = filterTabs(groups.flatMap((g) => g.tabs), query)
  return tabs.length ? [{ id: 'matches', label: 'Matches', tabs }] : []
}

/** The first room that matches, in the order the tiles are shown (what Enter opens). */
export function firstMatch(groups) {
  return groups[0]?.tabs[0] ?? null
}

/**
 * Where the keyboard focus goes. `sizes` is how many tiles each group holds, `at` is { g, i } (the group
 * and the tile in it), `cols` is how many tiles fit across. Left and Right walk through every tile and wrap;
 * Down and Up keep the column, step from group to group, stop at the end of a short last row, and wrap.
 */
export function moveFocus(sizes, at, key, cols) {
  const groups = sizes.map((n, g) => ({ g, n })).filter((s) => s.n > 0).map((s) => s.g)
  if (!groups.length) return null
  const flat = groups.flatMap((g) => Array.from({ length: sizes[g] }, (_, i) => ({ g, i })))
  const here = Math.max(0, flat.findIndex((p) => p.g === at?.g && p.i === at?.i))
  const col = Math.max(1, cols || 1)
  if (key === 'ArrowLeft') return flat[(here - 1 + flat.length) % flat.length]
  if (key === 'ArrowRight') return flat[(here + 1) % flat.length]
  if (key === 'Home') return flat[0]
  if (key === 'End') return flat[flat.length - 1]
  const { g, i } = flat[here]
  const column = i % col
  if (key === 'ArrowDown') {
    if (i + col < sizes[g]) return { g, i: i + col }
    // a short last row: stop at its end when this is not the last row yet
    if (Math.floor(i / col) < Math.floor((sizes[g] - 1) / col)) return { g, i: sizes[g] - 1 }
    const next = groups[(groups.indexOf(g) + 1) % groups.length]
    return { g: next, i: Math.min(column, sizes[next] - 1) }
  }
  if (key === 'ArrowUp') {
    if (i - col >= 0) return { g, i: i - col }
    const prev = groups[(groups.indexOf(g) - 1 + groups.length) % groups.length]
    const lastRow = Math.floor((sizes[prev] - 1) / col) * col
    return { g: prev, i: Math.min(lastRow + column, sizes[prev] - 1) }
  }
  return flat[here]
}

const BAD = ['failed', 'timed_out', 'delivery_failed']
const WEEK = 7 * 24 * 3600 * 1000
const plural = (n, one, many) => (n === 1 ? `1 ${one}` : `${n} ${many}`)
const isoDay = (d) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`

/**
 * What each tile says is waiting, read from state Castle already holds (nothing is fetched here):
 * { roomId: { count, tone: 'bad' | 'warn' | 'info', text } }. A count of 0 is a plain dot.
 */
export function badgesFor({ automations = null, pending = 0, marks = [], calendar = null, browser = null } = {}, now = new Date()) {
  const out = {}
  const titles = { chat: 'New in the room', story: 'The story has new lines' }
  for (const id of marks) out[id] = { count: 0, tone: 'info', text: titles[id] || 'Something new is waiting' }
  if (automations) {
    const recent = (automations.notices || []).filter((n) => {
      const at = Date.parse(String(n.at || '').replace(' ', 'T'))
      return BAD.includes(n.status) && Number.isFinite(at) && now - at <= WEEK
    }).length
    if (recent) out.automations = { count: recent, tone: 'bad', text: `${plural(recent, 'recent job failed or did not deliver', 'recent jobs failed or did not deliver')}` }
    else if (automations.paused) out.automations = { count: 0, tone: 'warn', text: 'Automations are paused' }
  }
  if (browser?.count > 0) out.browser = { count: browser.count, tone: 'info', text: browser.title ? `New in your tabs: ${browser.title}` : 'New in your tabs' }
  if (pending > 0) out.messaging = { count: pending, tone: 'warn', text: `${plural(pending, 'chat is', 'chats are')} waiting for your approval` }
  if (calendar) {
    const today = isoDay(now)
    const events = (calendar.next || []).filter((i) => i.date === today).length
    const reminders = (calendar.notices || []).length
    const suggestions = calendar.proposed || 0
    const parts = [events && `${plural(events, 'event', 'events')} today`, reminders && plural(reminders, 'reminder', 'reminders'), suggestions && plural(suggestions, 'suggestion waiting', 'suggestions waiting')].filter(Boolean)
    if (parts.length) out.calendar = { count: events + reminders + suggestions, tone: 'info', text: parts.join(', ') }
  }
  return out
}

/**
 * Does this key press open (or close) APPS? Ctrl+K, or Cmd+K on a Mac, and nothing else held. It is left
 * alone in DEV, where Ctrl+K belongs to the shell in the terminal ("delete to the end of the line"), and
 * while a sheet is open over Castle (Settings, the Sentinel, the backup panel), which keeps the keyboard.
 */
export function opensLauncher(event, { room = '', sheetOpen = false } = {}) {
  const key = String(event?.key ?? '').toLowerCase()
  if (key !== 'k' || !(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey || event.repeat) return false
  return room !== 'dev' && !sheetOpen
}
