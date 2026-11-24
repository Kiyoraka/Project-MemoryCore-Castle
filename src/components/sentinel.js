// The Sentinel's page logic, kept pure so it is tested: the shield's look from the status, and
// which buttons each finding shows.

export const STALE_MINUTES = 120
export const MAC_LINE = 'The Sentinel keeps watch on Windows for now.'
// the watch ticks every five minutes: three missed ticks mean it is not running
export const WATCH_LATE_MINUTES = 15

// The shield's state in words.
export const LOOK_WORDS = { quiet: 'All quiet', look: 'Worth a look', alarm: 'Alarm', unset: 'Not set up', away: 'Windows for now' }

// How long ago the companion's last patrol ran.
export function patrolAgo(status) {
  const p = status?.patrol
  if (!p) return 'No patrol yet.'
  if (p.minutesAgo < 1) return 'Last patrol just now.'
  if (p.minutesAgo < 120) return `Last patrol ${p.minutesAgo} min ago.`
  return `Last patrol ${Math.round(p.minutesAgo / 60)} h ago.`
}

// How long ago the five-minute watch ran, and whether that is late (it is not running).
// -> { line, late }
export function watchAgo(status) {
  const m = status?.watchMinutes
  if (m === null || m === undefined) return { line: 'The watch has not run yet.', late: false }
  const late = m > WATCH_LATE_MINUTES
  if (m < 1) return { line: 'The watch ran just now.', late }
  if (m < 120) return { line: `The watch ran ${m} min ago.`, late }
  return { line: `The watch ran ${Math.round(m / 60)} h ago.`, late }
}

// -> { look: 'away' | 'unset' | 'quiet' | 'look' | 'alarm', line, stale }
export function shieldLook(status) {
  if (!status) return { look: 'unset', line: 'The Sentinel is waking up.', stale: false }
  if (!status.supported) return { look: 'away', line: MAC_LINE, stale: false }
  if (!status.on) return { look: 'unset', line: 'The Sentinel is off. Turn it on in Settings > Sentinel.', stale: false }
  if (!status.installed) return { look: 'unset', line: 'Run the set-up line in Settings > Sentinel to let it keep watch.', stale: false }
  const patrol = status.patrol
  const stale = !patrol || patrol.minutesAgo > STALE_MINUTES
  const open = status.open || { alarm: 0, look: 0, note: 0 }
  let look = 'quiet'
  if (status.unread > 0 || open.alarm > 0 || patrol?.status === 'alarm') look = 'alarm'
  else if (!status.current || patrol?.status === 'look' || open.look > 0) look = 'look'
  let line = patrol?.line || 'No patrol yet. It runs within the hour, or press Patrol now.'
  if (!status.current) line = status.setupLine
  else if (status.problem) line = status.problem
  if (status.patrolling) line = 'On patrol now…'
  return { look, line, stale }
}

// The buttons a finding shows: Fix (only with a move), This is normal, Undo (only once fixed by a move).
// The sheet's one line about the Aegis Network: joined or not, and today's blocks.
export function networkLine(status) {
  if (!status?.network) return 'The Aegis Network: not joined (Settings > Sentinel).'
  const n = status.networkBlocked ?? 0
  return n ? `The Aegis Network: joined, ${n} address${n === 1 ? '' : 'es'} blocked today.` : 'The Aegis Network: joined. Nothing blocked today.'
}

export function findingButtons(finding) {
  if (!finding) return []
  if (finding.state === 'open') {
    const out = []
    if (finding.fixMove) out.push('fix')
    out.push('accept')
    return out
  }
  if (finding.state === 'fixed' && finding.undoable) return ['undo']
  return []
}

// Words for a move, as the member reads them.
const MOVES = {
  'firewall-on': 'Turn the firewall on',
  'protection-on': 'Turn protection on',
  'scan-quick': 'Run a quick scan',
  'signatures-update': 'Update the antivirus',
  'block-address': 'Block this address for a day',
  'block-port': 'Block this port',
  'disable-task': 'Switch this task off',
  'disable-startup': 'Stop it starting with Windows',
  'stop-process': 'Stop this program',
}
export const moveLabel = (move) => MOVES[move] || 'Fix'
