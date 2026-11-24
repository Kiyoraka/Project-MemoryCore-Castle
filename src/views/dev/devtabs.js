// The DEV room's pure rules: which tab shows, how the project list reads, and how a plan's
// boxes are grouped and marked. No calls to the core here.

export const MAX_TABS = 8

// A folder compared the Windows way: either slash, no trailing slash, any case.
export function samePath(a, b) {
  const norm = (p) => String(p ?? '').replace(/\//g, '\\').replace(/\\+$/, '').toLowerCase()
  return !!a && !!b && norm(a) === norm(b)
}

// The last part of a folder, for a tab with no name.
export function folderName(folder) {
  const parts = String(folder ?? '').split(/[\\/]+/).filter(Boolean)
  return parts[parts.length - 1] ?? ''
}

// The open tabs in slot order (0 first).
export function orderTabs(tabs) {
  return [...(tabs ?? [])].filter((t) => Number.isInteger(t?.slot)).sort((a, b) => a.slot - b.slot)
}

export function tabForSlot(tabs, slot) {
  return (tabs ?? []).find((t) => t.slot === slot) ?? null
}

export function tabForFolder(tabs, folder) {
  return (tabs ?? []).find((t) => samePath(t.folder, folder)) ?? null
}

// The tabs of one project's sessions, by their number (shop · 1 first).
export function sessionsOf(tabs, folder) {
  return (tabs ?? []).filter((t) => samePath(t.folder, folder)).sort((a, b) => (a.number ?? 0) - (b.number ?? 0))
}

// A tab's name in the strip: its project and its number there, fixed for the session's life.
export function tabLabel(tab) {
  if (!tab) return ''
  const name = tab.name || folderName(tab.folder)
  return tab.number ? `${name} · ${tab.number}` : name
}

// The projects on the list, without the memory folder (it has its own session at HOME). Each
// carries its sessions' slots (by number), how many there are, the newest one's slot (or null)
// and whether its folder is gone from this computer.
export function recentProjects(projects, tabs, memoryFolder) {
  return (projects ?? [])
    .filter((p) => p?.folder && !samePath(p.folder, memoryFolder))
    .map((p) => {
      const slots = sessionsOf(tabs, p.folder).map((t) => t.slot)
      return {
        folder: p.folder,
        name: p.name || folderName(p.folder),
        missing: !!p.missing,
        slots,
        count: slots.length,
        slot: slots.length ? slots[slots.length - 1] : null,
      }
    })
}

// The tab a new session opened in: the slot that is open now and was not before (or null).
export function newSlot(before, after) {
  const was = new Set((before ?? []).map((t) => t.slot))
  return (after ?? []).find((t) => !was.has(t.slot))?.slot ?? null
}

// What "Remove project…" asks: plain when nothing runs, naming each session that will end otherwise.
export function removeQuestion(project, tabs) {
  const ending = sessionsOf(tabs, project.folder)
  if (!ending.length) {
    return { title: `Remove ${project.name}?`, text: 'It comes off the list. The folder and its files stay on disk, and you can open it again any time.', button: 'Remove project' }
  }
  const names = ending.map(tabLabel).join(', ')
  const many = ending.length > 1
  return {
    title: `Remove ${project.name}?`,
    text: `Its ${ending.length} ${many ? 'sessions' : 'session'} will end (${names}). The folder stays on disk.`,
    button: many ? `End all ${ending.length} and remove` : 'End it and remove',
  }
}

// Which tab shows after the tabs changed: the one asked for (by folder, its newest session), else the current one if it
// is still open, else its nearest neighbour (the next slot up, then the one below), else none.
export function nextActive(tabs, current, wantedFolder = null) {
  const list = orderTabs(tabs)
  if (wantedFolder) {
    const wanted = sessionsOf(list, wantedFolder)
    if (wanted.length) return wanted[wanted.length - 1].slot
  }
  if (!list.length) return null
  if (current === null || current === undefined) return list[0].slot
  if (list.some((t) => t.slot === current)) return current
  const after = list.find((t) => t.slot > current)
  if (after) return after.slot
  return list[list.length - 1].slot
}

// What a session panel shows for a tab; a tab that is no longer open reads as asleep.
export function sessionView(tab) {
  if (!tab) return { running: false, status: { ready: false, message: 'This tab is closed.', fix: null } }
  return {
    running: !!tab.running,
    status: { ready: !!tab.status?.ready, message: tab.status?.message ?? '', fix: tab.status?.fix ?? null },
    notice: tab.notice ?? null,
  }
}

// The chat room name of a DEV tab ('dev:3'), and the slot back from it (null for any other room).
export function devRoom(slot) {
  return `dev:${slot}`
}

export function devSlotOf(room) {
  const match = /^dev:([0-9]+)$/.exec(typeof room === 'string' ? room : '')
  if (!match) return null
  const slot = Number(match[1])
  return slot < MAX_TABS ? slot : null
}

// A message typed into a DEV session: one bracketed paste, so a new line never sends it early.
export function pasted(text) {
  return `\u001b[200~${text}\u001b[201~`
}

// ---------- Plans ----------

const CLOSED_PLAN = new Set(['done', 'closed', 'complete', 'completed', 'finished', 'archived', 'superseded'])

export function openPlans(plans) {
  return (plans ?? []).filter((p) => !CLOSED_PLAN.has(String(p?.status ?? '').toLowerCase()))
}

// A plan's boxes in order, cut into groups at each phase row: [{ heading, items }]. Boxes before the
// first phase row sit in a group with no heading.
export function groupByPhase(items) {
  const sorted = [...(items ?? [])].sort((a, b) => (a.ordinal ?? 0) - (b.ordinal ?? 0))
  const groups = []
  let group = null
  for (const item of sorted) {
    if (item.kind === 'phase') {
      group = { heading: item.text, state: item.state ?? 'open', items: [] }
      groups.push(group)
      continue
    }
    if (!group) {
      group = { heading: null, state: 'open', items: [] }
      groups.push(group)
    }
    group.items.push(item)
  }
  return groups.filter((g) => g.heading !== null || g.items.length)
}

const MARKS = {
  done: { mark: '✓', label: 'Done' },
  carried: { mark: '–', label: 'Carried to a later plan' },
  flagged: { mark: '⚑', label: 'Flagged' },
  open: { mark: '', label: 'Open' },
}

// The mark in an item's box, and what it means (for its tooltip).
export function itemMark(state) {
  return MARKS[state] ?? MARKS.open
}

// "3 / 7": the boxes done out of all boxes (phase rows are headings, not boxes).
export function planProgress(items) {
  const boxes = (items ?? []).filter((i) => i.kind !== 'phase')
  const done = boxes.filter((i) => i.state === 'done').length
  return boxes.length ? `${done} / ${boxes.length}` : ''
}
