import { folderName, tabLabel } from '../views/dev/devtabs.js'

// DEV-tab chips in HOME's chat: a companion line about one of the DEV tabs carries a small chip
// naming the tab and what it wants (needs your go, finished, FYI); a click opens that tab.
// Two ways tell Castle which tab a line is about:
// - the marker the companion writes at the end of the line, `[dev:shop|done]` (taught in the wake
//   prompt, mirror.rs DEV_MARKER_RULE). Castle hides it; it carries the state.
// - the trail: HOME sent a message to a DEV tab's session (SendMessage) or heard from one (a
//   cross-session message). The transcript gives these as "peer" entries (transcript/claude.rs);
//   the companion's next words get an FYI chip for that tab, even without a marker.
// Pure; ChatView and ChatEntries draw it.

export const DEV_STATES = {
  go: 'needs your go',
  done: 'finished',
  fyi: 'FYI',
}

// [dev:<tab>] or [dev:<tab>|go|done|fyi]; the tab's name has no brackets, bars, backticks or line breaks
const MARKER = /[ \t]*\[dev:[ \t]*([^[\]|\n`]{1,80}?)[ \t]*(?:\|[ \t]*(go|done|fyi)[ \t]*)?\]/gi
const FENCE = /^ {0,3}(```|~~~)/

/**
 * The companion's words with each DEV marker taken out, and the markers found ({ name, state }),
 * in order, one per tab. Markers inside a fenced code block or an inline `code` span are left as
 * written; so is anything that is not a whole marker ([dev:], [dev:shop|maybe], [dev:shop).
 */
export function parseDevMarkers(text) {
  const source = String(text ?? '')
  if (!/\[dev:/i.test(source)) return { text: source, markers: [] }
  const markers = []
  const seen = new Set()
  let fenced = null
  const lines = source.split('\n').map((line) => {
    const fence = line.match(FENCE)
    if (fence) {
      if (!fenced) fenced = fence[1]
      else if (fence[1] === fenced) fenced = null
      return line
    }
    if (fenced) return line
    // inline code spans stay as written: only the parts between them are read
    const changed = line
      .split(/(`+[^`]*`+)/)
      .map((part, n) =>
        n % 2
          ? part
          : part.replace(MARKER, (_, name, state) => {
              const key = devKey(name)
              if (!seen.has(key)) {
                seen.add(key)
                markers.push({ name: name.trim(), state: (state ?? 'fyi').toLowerCase() })
              }
              return ''
            }),
      )
      .join('')
    return changed === line ? line : changed.replace(/[ \t]+$/, '')
  })
  return { text: markers.length ? lines.join('\n').replace(/\n+$/, '') : source, markers }
}

/** A tab's name as it is compared: any case, "shop · 2", "shop 2", "shop_2" and "shop-2" the same. */
export function devKey(name) {
  return String(name ?? '')
    .toLowerCase()
    .replace(/[\s·_\-–—.]+/g, '-')
    .replace(/^-+|-+$/g, '')
}

/** The process id in a Claude Code session address (`uds:/tmp/cc-socks/<pid>.sock`), or null. */
export function pidOf(address) {
  const found = String(address ?? '').match(/cc-socks(?:-\d+)?[\\/](\d+)\.sock\b/)
  return found ? Number(found[1]) : null
}

/**
 * Who a "peer" entry names: { name, pid }. A message sent says `to` (a name, "name [ref]", or an
 * address); one heard says its sender's name and address, one per line.
 */
export function peerOf(entry) {
  if (entry?.role !== 'peer') return null
  const [first = '', second = ''] = String(entry.text ?? '').split('\n')
  const address = entry.state === 'from' ? second : /^(uds|bridge):/i.test(first.trim()) ? first : ''
  const name = address === first ? '' : first.replace(/\s*\[[\w-]+\]\s*$/, '').trim()
  return { name, pid: pidOf(address) }
}

// how a chip names a tab: the project alone while it has one session, with its number otherwise
function chipLabel(tab, tabs) {
  const name = tab.name || folderName(tab.folder)
  const siblings = tabs.filter((t) => devKey(t.name || folderName(t.folder)) === devKey(name))
  return siblings.length > 1 ? tabLabel(tab) : name
}

/**
 * The DEV tab a name or a process id means, from DEV's state ({ tabs, projects }):
 * { slot, folder, label, open: true } for an open tab, { slot: null, folder, label, open: false }
 * for a project with no session open now, or null when it is no DEV project at all.
 * A process id is exact; a name is the tab's ("shop · 2"), or its project's (then its first session).
 */
export function findDevTab({ name = '', pid = null } = {}, dev = {}) {
  const tabs = [...(dev.tabs ?? [])].filter((t) => Number.isInteger(t?.slot)).sort((a, b) => (a.number ?? 0) - (b.number ?? 0) || a.slot - b.slot)
  const projects = dev.projects ?? []
  const open = (tab) => ({ slot: tab.slot, folder: tab.folder, label: chipLabel(tab, tabs), open: true })
  if (pid) {
    const byPid = tabs.find((t) => t.pid === pid)
    if (byPid) return open(byPid)
  }
  const key = devKey(name)
  if (!key) return null
  const project = (t) => devKey(t.name || folderName(t.folder))
  const exact = tabs.find((t) => devKey(tabLabel(t)) === key)
  if (exact) return open(exact)
  const first = tabs.find((t) => project(t) === key)
  if (first) return open(first)
  // a project whose sessions are all closed (by its name, or its name and a number)
  const closed = projects.find((p) => {
    const own = devKey(p.name || folderName(p.folder))
    return own === key || new RegExp(`^${own.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}-\\d+$`).test(key)
  })
  if (closed) return { slot: null, folder: closed.folder, label: closed.name || folderName(closed.folder), open: false }
  return null
}

/**
 * The trail: which companion message each "peer" entry belongs to, as entry id -> [{ name, pid }].
 * A message sent or heard goes on the companion's next words; when none follow before the member
 * speaks again (or yet), on her last words since the member last spoke.
 */
export function devTrail(entries) {
  const trail = new Map()
  let pending = []
  let lastWords = null
  const put = (id) => {
    if (!id || !pending.length) return
    trail.set(id, [...(trail.get(id) ?? []), ...pending])
    pending = []
  }
  for (const entry of entries ?? []) {
    if (entry.role === 'peer') {
      const peer = peerOf(entry)
      if (peer && (peer.name || peer.pid)) pending.push(peer)
    } else if (entry.role === 'companion' && String(entry.text ?? '').trim()) {
      lastWords = entry.id
      put(entry.id)
    } else if (entry.role === 'member') {
      put(lastWords)
      pending = []
      lastWords = null
    }
  }
  put(lastWords)
  return trail
}

/**
 * The chips on one companion message: its markers first (they carry the state), then the trail's
 * tabs as FYI, one chip per tab. Each chip: { key, slot, folder, label, state, open, from }.
 * A marker naming no DEV project still shows, as a chip that opens nothing; a trail to a session
 * that is no DEV tab shows none.
 */
export function chipsFor(id, markers, peers, dev) {
  const chips = []
  const has = (place) => chips.some((c) => (place.slot !== null ? c.slot === place.slot : c.folder && c.folder === place.folder))
  for (const marker of markers ?? []) {
    const place = findDevTab({ name: marker.name }, dev) ?? { slot: null, folder: null, label: marker.name, open: false, unknown: true }
    if (!place.unknown && has(place)) continue
    chips.push({ ...place, state: marker.state, from: 'marker', key: `${id}:${place.slot ?? place.folder ?? devKey(marker.name)}` })
  }
  for (const peer of peers ?? []) {
    const place = findDevTab(peer, dev)
    if (!place || has(place)) continue
    chips.push({ ...place, state: 'fyi', from: 'trail', key: `${id}:${place.slot ?? place.folder}` })
  }
  return chips
}

/**
 * The chat's items with DEV chips: each companion message gets `shown` (its words without the
 * markers) and `dev` (its chips). `chips: false` (rooms other than HOME) only hides the markers.
 */
export function withDevChips(items, entries, dev, { chips = true } = {}) {
  const trail = chips ? devTrail(entries) : new Map()
  return (items ?? []).map((item) => {
    if (item.kind !== 'message' || item.entry?.role !== 'companion') return item
    const { text, markers } = parseDevMarkers(item.entry.text)
    const peers = trail.get(item.entry.id)
    if (!markers.length && !peers?.length) return item
    const found = chips ? chipsFor(item.entry.id, markers, peers, dev) : []
    return { ...item, shown: text, dev: found }
  })
}

/** What a chip says after the tab's name: its state, or that the tab is closed or unknown. */
export function chipWords(chip) {
  if (chip.unknown) return 'no DEV tab by that name'
  if (!chip.open) return 'closed'
  return DEV_STATES[chip.state] ?? DEV_STATES.fyi
}

/** What a chip says on hover. */
export function chipTitle(chip) {
  if (chip.unknown) return `No DEV tab or project is called ${chip.label}`
  if (!chip.open) return `${chip.label} has no session open - opens the project in DEV`
  return `Open ${chip.label} in DEV`
}

/**
 * The chips the DEV button counts: needs your go and finished, on an open tab, said since `since`
 * (ms; Castle's start, so an old talk read again never counts) and not seen yet. [{ key, slot }]
 */
export function badgeChips(items, since, seen = new Set()) {
  const out = []
  for (const item of items ?? []) {
    const at = Date.parse(item.entry?.at ?? '')
    if (!item.dev?.length || Number.isNaN(at) || at < since) continue
    for (const chip of item.dev) {
      if (chip.open && (chip.state === 'go' || chip.state === 'done') && !seen.has(chip.key)) out.push({ key: chip.key, slot: chip.slot })
    }
  }
  return out
}
