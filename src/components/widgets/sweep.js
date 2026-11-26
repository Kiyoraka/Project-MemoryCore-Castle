// SWEEP's words and numbers, apart from the page so they can be tested: sizes, the bar's parts,
// "last swept", the result line, the sheet's starting ticks and which of the three looks shows.

export const LOOKS = ['ring', 'bar', 'broom']
export const LOOK_NAMES = { ring: 'Ring', bar: 'Bar', broom: 'Broom' }
// one colour per group, from the theme's own palette where it has one
export const GROUP_COLORS = {
  temp: 'var(--sweep-temp, #a78bfa)',
  browser: 'var(--sweep-browser, #38bdf8)',
  thumbs: 'var(--sweep-thumbs, #818cf8)',
  crash: 'var(--sweep-crash, #60a5fa)',
  rust: 'var(--sweep-rust, #f0a35e)',
  recycle: 'var(--sweep-recycle, #34d399)',
}
export const GROUP_ICONS = { temp: '🗎', browser: '🌐', thumbs: '🖼', crash: '🗒', rust: '⚙', recycle: '🗑' }

// 6_871_947_673 -> "6.4 GB"; 850 MB; 12 KB; 0 B
export function sizeLabel(bytes) {
  const n = Number(bytes) || 0
  if (n < 1024) return `${n} B`
  const units = ['KB', 'MB', 'GB', 'TB']
  let value = n / 1024
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  return `${value >= 100 || unit === 0 ? Math.round(value) : value.toFixed(1)} ${units[unit]}`
}

// The bytes of the groups given (all of them when no ids).
export function totalOf(groups = [], ids = null) {
  return groups.filter((g) => !ids || ids.has(g.id)).reduce((sum, g) => sum + (Number(g.bytes) || 0), 0)
}

// The bar's coloured parts: each group's share of the whole, empty groups left out.
export function barParts(groups = []) {
  const total = totalOf(groups)
  if (!total) return []
  return groups.filter((g) => g.bytes > 0).map((g) => ({ id: g.id, share: g.bytes / total, color: GROUP_COLORS[g.id] ?? 'var(--accent)' }))
}

// "Last swept 3 days ago" from unix seconds; "Not swept yet" without one.
export function sweptLine(unix, nowMs = Date.now()) {
  if (!unix) return 'Not swept yet'
  const minutes = Math.round((nowMs - unix * 1000) / 60000)
  if (minutes < 2) return 'Last swept just now'
  if (minutes < 60) return `Last swept ${minutes} min ago`
  const hours = Math.round(minutes / 60)
  if (hours < 48) return `Last swept ${hours} h ago`
  return `Last swept ${Math.round(hours / 24)} days ago`
}

// The look that shows: the member's choice, else the theme's, else the bar.
export function resolveLook(memberLook, themeLook) {
  if (LOOKS.includes(memberLook)) return memberLook
  if (LOOKS.includes(themeLook)) return themeLook
  return 'bar'
}

// The groups a sheet always opens unticked: the Rust build caches (the next build of each project
// starts from scratch). The Recycle Bin follows the gear.
export const UNTICKED_FIRST = ['rust']

// The sheet starts with every group ticked but the Recycle Bin (unless the gear says otherwise) and
// the UNTICKED_FIRST groups.
export function startingTicks(groups = [], recycleBin = false) {
  return new Set(groups.filter((g) => !UNTICKED_FIRST.includes(g.id) && (g.id !== 'recycle' || recycleBin)).map((g) => g.id))
}

// "Freed 5.9 GB - 214 files skipped (in use)" from the groups' results.
export function resultLine(results = []) {
  const freed = results.reduce((sum, r) => sum + (Number(r.freed) || 0), 0)
  const skipped = results.reduce((sum, r) => sum + (Number(r.skipped) || 0), 0)
  const head = `Freed ${sizeLabel(freed)}`
  return skipped ? `${head} - ${skipped} ${skipped === 1 ? 'file' : 'files'} skipped (in use)` : head
}
