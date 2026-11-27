// SWEEP's Developer section, its words and numbers apart from the page so they can be tested: the
// starting picks, what a pick frees, each group's summary line, the section's line, the worktree
// line, the confirm and result lines, and HOME's disk warning.
import { sizeLabel } from './sweep.js'

// the groups inside the section, in order
export const DEV_GROUPS = [
  { id: 'default', title: 'On by default' },
  { id: 'worktrees', title: 'Worktrees' },
  { id: 'projects', title: 'Project build output' },
]
export const CHOICE_NAMES = { leave: 'Leave', build: 'Build output', whole: 'Whole worktree' }
// HOME's Sweep card warns from here
export const DISK_WARN_PERCENT = 90

const isWorktree = (item) => item.kind === 'worktree'

// Everything marked on (and not in use) starts ticked; the rest, worktrees too, starts on Leave.
export function startingPicks(items = []) {
  const picks = {}
  for (const item of items) picks[item.id] = item.on && !item.blocked ? 'clean' : 'leave'
  return picks
}

// What one item frees with the pick given; nothing when it is disabled or the pick is not offered.
export function pickedBytes(item, how) {
  if (!item || item.blocked || !how || how === 'leave') return 0
  if (isWorktree(item)) {
    if (!item.choices?.includes(how)) return 0
    return Number(how === 'whole' ? item.wholeBytes : item.bytes) || 0
  }
  return how === 'clean' ? Number(item.bytes) || 0 : 0
}

// The picks to send: only what is offered, ticked and not disabled.
export function picksToSend(items = [], picks = {}) {
  return items
    .filter((item) => !item.blocked && picks[item.id] && picks[item.id] !== 'leave')
    .filter((item) => (isWorktree(item) ? item.choices?.includes(picks[item.id]) : picks[item.id] === 'clean'))
    .map((item) => ({ id: item.id, how: picks[item.id] }))
}

export function tickedTotal(items = [], picks = {}) {
  return items.reduce((sum, item) => sum + pickedBytes(item, picks[item.id]), 0)
}

// "4 items · 12.3 GB ticked · 2 in use or changed"
export function groupSummary(items = [], picks = {}) {
  const count = items.length
  const held = items.filter((item) => item.blocked || item.dirty).length
  const head = `${count} ${count === 1 ? 'item' : 'items'} · ${sizeLabel(tickedTotal(items, picks))} ticked`
  return held ? `${head} · ${held} in use or changed` : head
}

// "14:52" in this computer's time
export function clockOf(unix) {
  const d = new Date(unix * 1000)
  return `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
}

// The section's one line: "Developer · 9.4 GB ready to free · scanned 14:52"
export function devSummary({ scanned = null, scanning = false, total = 0 } = {}) {
  if (scanning) return 'Developer · scanning…'
  if (!scanned) return 'Developer · not scanned yet'
  return `Developer · ${sizeLabel(total)} ready to free · scanned ${clockOf(scanned)}`
}

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec']

// "branch feat/x · merged into main · last commit 2 Nov" and the scan's own note after it
export function worktreeLine(item) {
  const parts = []
  parts.push(item.branch ? `branch ${item.branch}` : 'no branch')
  if (item.mergedInto) parts.push(`merged into ${item.mergedInto}`)
  if (item.lastCommit) {
    const d = new Date(item.lastCommit * 1000)
    parts.push(`last commit ${d.getDate()} ${MONTHS[d.getMonth()]}`)
  }
  // the scan's note: "pushed, not merged · build output only", "uncommitted changes · …"
  if (item.detail) parts.push(item.detail)
  return parts.join(' · ')
}

// "Clean 3 items, 12.4 GB?"
export function confirmLine(items = [], picks = {}) {
  const sent = picksToSend(items, picks)
  if (!sent.length) return 'Nothing is ticked.'
  const total = tickedTotal(items, picks)
  const unknown = sent.some((p) => items.find((i) => i.id === p.id)?.bytesKnown === false)
  return `Clean ${sent.length} ${sent.length === 1 ? 'item' : 'items'}, ${sizeLabel(total)}${unknown ? ' and more' : ''}?`
}

// "Freed 12.4 GB from 3 items - 1 stayed (in use: node)"
export function devResultLine(results = []) {
  const done = results.filter((r) => r.ok)
  const freed = done.reduce((sum, r) => sum + (Number(r.freed) || 0), 0)
  const head = `Freed ${sizeLabel(freed)} from ${done.length} ${done.length === 1 ? 'item' : 'items'}`
  const stayed = results.filter((r) => !r.ok)
  if (!stayed.length) return head
  const why = stayed.length === 1 && stayed[0].note ? ` (${stayed[0].note})` : ''
  return `${head} - ${stayed.length} stayed${why}`
}

// The system drive ('/' or C:), else the first fixed drive: "Disk 92% full" from 90%, else null.
export function diskWarning(drives = []) {
  const drive = drives.find((d) => d.mount === '/' || /^c:/i.test(d.mount)) ?? drives.find((d) => !d.removable)
  if (!drive || !drive.total) return null
  const percent = Math.round(((drive.total - drive.free) / drive.total) * 100)
  return percent >= DISK_WARN_PERCENT ? { percent, line: `Disk ${percent}% full` } : null
}
