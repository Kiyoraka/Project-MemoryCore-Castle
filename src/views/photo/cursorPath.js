// PHOTO: the path math behind her cursor. Points are canvas pixels, [x, y] or { x, y }.

export const MIN_MS = 600
export const MAX_MS = 2000

const num = (v) => (typeof v === 'number' && Number.isFinite(v) ? v : null)

// Every readable point as { x, y }; the rest dropped.
export function normPoints(points) {
  const out = []
  for (const p of Array.isArray(points) ? points : []) {
    const x = num(Array.isArray(p) ? p[0] : p?.x)
    const y = num(Array.isArray(p) ? p[1] : p?.y)
    if (x !== null && y !== null) out.push({ x, y })
  }
  return out
}

export function pathLength(points) {
  const pts = normPoints(points)
  let total = 0
  for (let i = 1; i < pts.length; i++) total += Math.hypot(pts[i].x - pts[i - 1].x, pts[i].y - pts[i - 1].y)
  return total
}

// The point a fraction t (0-1) of the way along the path, by distance; null for no points.
export function pointAt(points, t) {
  const pts = normPoints(points)
  if (!pts.length) return null
  if (pts.length === 1) return { ...pts[0] }
  const f = Math.min(1, Math.max(0, Number.isFinite(t) ? t : 0))
  const total = pathLength(pts)
  if (total === 0) return { ...pts[0] }
  let want = f * total
  for (let i = 1; i < pts.length; i++) {
    const a = pts[i - 1]
    const b = pts[i]
    const seg = Math.hypot(b.x - a.x, b.y - a.y)
    if (want <= seg || i === pts.length - 1) {
      const k = seg === 0 ? 0 : Math.min(1, want / seg)
      return { x: a.x + (b.x - a.x) * k, y: a.y + (b.y - a.y) * k }
    }
    want -= seg
  }
  return { ...pts[pts.length - 1] }
}

// How long the glide takes for a path this long (screen px): 600 ms to 2 s.
export function durationFor(length) {
  const l = Math.max(0, Number.isFinite(length) ? length : 0)
  return Math.round(Math.min(MAX_MS, MIN_MS + l * 1.5))
}

// Slow at both ends.
export function ease(t) {
  const f = Math.min(1, Math.max(0, t))
  return f < 0.5 ? 2 * f * f : 1 - Math.pow(-2 * f + 2, 2) / 2
}
