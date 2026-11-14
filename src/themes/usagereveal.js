// A theme may name a "low" picture (`usageReveal`): its wallpaper as it looks when none of the
// week's limit is used. HOME lays the real wallpaper over it and reveals it as the limit is used,
// so the picture fills with the week: the Milky Way rises, the crystal wakes, the tree lights from
// its roots. Every percent shows, not only a few steps.
//   rise: the wallpaper shows up to an edge that moves along `angle` from `from` to `to`
//         (fractions along the CSS gradient line of the wallpaper's own box)
//   fade: the wallpaper's opacity follows the percent

const SOFT = 0.06 // the soft edge of a rise, as a fraction of the gradient line

const fraction = (v, fallback) => {
  const n = Number(v)
  return Number.isFinite(n) ? Math.min(1, Math.max(0, n)) : fallback
}

// The theme's usageReveal made safe, or null when it cannot be used.
export function readReveal(reveal) {
  if (!reveal || typeof reveal !== 'object' || !reveal.lowUrl) return null
  if (reveal.mode === 'fade') return { mode: 'fade', lowUrl: reveal.lowUrl }
  if (reveal.mode !== 'rise') return null
  const angle = /^-?\d+(\.\d+)?deg$/.test(String(reveal.angle ?? '')) ? reveal.angle : '0deg'
  const from = fraction(reveal.from, 0)
  const to = fraction(reveal.to, 1)
  return { mode: 'rise', lowUrl: reveal.lowUrl, angle, from: Math.min(from, to), to: Math.max(from, to) }
}

// How the wallpaper layer is shown at `usedPercent` (0..100): { opacity } for fade, { mask } for
// rise (an empty mask = the whole wallpaper).
export function revealStyle(usedPercent, reveal) {
  const used = Math.min(100, Math.max(0, Number(usedPercent) || 0)) / 100
  if (reveal.mode === 'fade') return { opacity: used }
  if (used <= 0) return { mask: 'linear-gradient(transparent, transparent)' }
  if (used >= 1) return { mask: '' }
  const edge = reveal.from + (reveal.to - reveal.from) * used
  const pct = (v) => `${(v * 100).toFixed(2)}%`
  return { mask: `linear-gradient(${reveal.angle}, #000 ${pct(Math.max(0, edge - SOFT))}, transparent ${pct(edge)})` }
}
