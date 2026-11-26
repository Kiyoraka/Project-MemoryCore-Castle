// Tiling maths for a zoomable page. Pure: no DOM, no canvas. Page space is points, y down, origin at
// the page's top-left, exactly as the Rust display list speaks (scriptorium/src/ground/display.rs).
//
// A tile is `tileSize` device pixels square; at a device scale of zoom x dpr it covers
// tileSize / (zoom x dpr) points of the page. The grid is anchored at the page origin, so tile
// (col, row) always lands on device pixel (col x tileSize, row x tileSize) relative to the page's
// device origin and the tiles meet without seams.

// ---------------------------------------------------------------------------------------------------
// Rects and matrices (the same maths as display.rs's Rect and Matrix)
// ---------------------------------------------------------------------------------------------------

/** `[x, y, w, h]`, `{x, y, w, h}` or null/undefined -> `{x, y, w, h}` or null. */
export function toRect(r) {
  if (!r) return null
  if (Array.isArray(r)) return r.length >= 4 ? { x: +r[0], y: +r[1], w: +r[2], h: +r[3] } : null
  if (typeof r === 'object' && 'x' in r) return { x: +r.x, y: +r.y, w: +(r.w ?? r.width ?? 0), h: +(r.h ?? r.height ?? 0) }
  return null
}

/** From left, top, right, bottom in any order. */
export function rectLtrb(l, t, r, b) {
  if (l > r) [l, r] = [r, l]
  if (t > b) [t, b] = [b, t]
  return { x: l, y: t, w: r - l, h: b - t }
}

/** Overlap or touch (a zero-height line on a tile's edge still counts), like Rect::intersects. */
export function intersects(a, b) {
  return a.x <= b.x + b.w && b.x <= a.x + a.w && a.y <= b.y + b.h && b.y <= a.y + a.h
}

export function intersection(a, b) {
  if (!intersects(a, b)) return null
  return rectLtrb(Math.max(a.x, b.x), Math.max(a.y, b.y), Math.min(a.x + a.w, b.x + b.w), Math.min(a.y + a.h, b.y + b.h))
}

export function union(a, b) {
  return rectLtrb(Math.min(a.x, b.x), Math.min(a.y, b.y), Math.max(a.x + a.w, b.x + b.w), Math.max(a.y + a.h, b.y + b.h))
}

export function inflate(r, d) {
  return { x: r.x - d, y: r.y - d, w: r.w + 2 * d, h: r.h + 2 * d }
}

export function containsPoint(r, x, y) {
  return x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h
}

export const IDENTITY = Object.freeze([1, 0, 0, 1, 0, 0])

/** `m x t`: applies `t` first, then `m` - the canvas's `ctx.transform(t)` on a context at `m`. */
export function multiply(m, t) {
  return [
    m[0] * t[0] + m[2] * t[1],
    m[1] * t[0] + m[3] * t[1],
    m[0] * t[2] + m[2] * t[3],
    m[1] * t[2] + m[3] * t[3],
    m[0] * t[4] + m[2] * t[5] + m[4],
    m[1] * t[4] + m[3] * t[5] + m[5],
  ]
}

export function applyPoint(m, x, y) {
  return [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]]
}

/** The box around the four transformed corners. */
export function applyRect(m, r) {
  const pts = [applyPoint(m, r.x, r.y), applyPoint(m, r.x + r.w, r.y), applyPoint(m, r.x, r.y + r.h), applyPoint(m, r.x + r.w, r.y + r.h)]
  let l = Infinity, t = Infinity, rr = -Infinity, b = -Infinity
  for (const [x, y] of pts) {
    if (x < l) l = x
    if (x > rr) rr = x
    if (y < t) t = y
    if (y > b) b = y
  }
  return { x: l, y: t, w: rr - l, h: b - t }
}

export function invert(m) {
  const det = m[0] * m[3] - m[1] * m[2]
  if (!det || !Number.isFinite(det)) return null
  const a = m[3] / det, b = -m[1] / det, c = -m[2] / det, d = m[0] / det
  return [a, b, c, d, -(a * m[4] + c * m[5]), -(b * m[4] + d * m[5])]
}

// ---------------------------------------------------------------------------------------------------
// Bounds, client side (the Rust side may supply them instead)
// ---------------------------------------------------------------------------------------------------

const NUM = /[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?/g

/**
 * Rough bounds of absolute SVG path data (`M L Q C Z`, as display.rs writes it): every number pair
 * is taken as a point, control points included, so the box is never too small. Null when empty.
 */
export function pathBounds(d) {
  if (!d) return null
  const nums = d.match(NUM)
  if (!nums || nums.length < 2) return null
  let l = Infinity, t = Infinity, r = -Infinity, b = -Infinity
  for (let i = 0; i + 1 < nums.length; i += 2) {
    const x = +nums[i], y = +nums[i + 1]
    if (x < l) l = x
    if (x > r) r = x
    if (y < t) t = y
    if (y > b) b = y
  }
  return { x: l, y: t, w: r - l, h: b - t }
}

/** How far a stroke can paint beyond the path's points (display.rs Stroke::reach). */
export function strokeReach(s) {
  if (!s) return 0
  const half = Math.abs(+s.w || 0) / 2
  let r = half
  if (s.cap === 'square') r = Math.max(r, half * Math.SQRT2)
  if ((s.join ?? 'miter') === 'miter') r = Math.max(r, half * Math.max(s.ml ?? 10, 1))
  return r
}

const STATE = new Set(['clip', 'unclip', 'transform', 'untransform', 'opacity', 'unopacity'])

export function isStateItem(item) {
  return !!item && STATE.has(item.t)
}

/** An item's box in its own coordinates (before pushed transforms), or null (state items, empty runs). */
export function itemBounds(item) {
  if (!item) return null
  switch (item.t) {
    case 'path': {
      const r = pathBounds(item.d)
      return r && item.stroke ? inflate(r, strokeReach(item.stroke)) : r
    }
    case 'glyphs': {
      const s = Math.abs(+item.size || 0)
      const g = item.g || []
      let out = null
      for (let i = 0; i + 2 < g.length; i += 3) {
        const x = g[i + 1], y = g[i + 2]
        const r = rectLtrb(x - 0.5 * s, y - 1.2 * s, x + 1.5 * s, y + 0.5 * s)
        out = out ? union(out, r) : r
      }
      return out
    }
    case 'image':
    case 'link': {
      const r = toRect(item.r)
      return r ? rectLtrb(r.x, r.y, r.x + r.w, r.y + r.h) : null
    }
    default:
      return null
  }
}

/**
 * Each item's box in page coordinates (pushed transforms applied, pushed clips intersected), or null
 * for state items and items that cannot be seen - display.rs DisplayList::world_bounds, client side.
 */
export function worldBounds(list) {
  const items = list?.items || []
  const out = new Array(items.length)
  let m = IDENTITY
  const mStack = []
  // undefined: no clip; null: clipped to nothing; rect: the clip box in page coordinates
  let clip
  const clipStack = []
  for (let i = 0; i < items.length; i++) {
    const it = items[i]
    switch (it?.t) {
      case 'transform':
        mStack.push(m)
        m = multiply(m, it.m || IDENTITY)
        out[i] = null
        break
      case 'untransform':
        m = mStack.pop() || IDENTITY
        out[i] = null
        break
      case 'clip': {
        clipStack.push(clip)
        const r = pathBounds(it.d)
        const wr = r ? applyRect(m, r) : null
        if (clip === undefined) clip = wr
        else clip = clip && wr ? intersection(clip, wr) : null
        out[i] = null
        break
      }
      case 'unclip':
        clip = clipStack.length ? clipStack.pop() : undefined
        out[i] = null
        break
      case 'opacity':
      case 'unopacity':
        out[i] = null
        break
      default: {
        const b = itemBounds(it)
        const wb = b ? applyRect(m, b) : null
        out[i] = !wb ? null : clip === undefined ? wb : clip ? intersection(wb, clip) : null
      }
    }
  }
  return out
}

/**
 * The indices of the items to paint for `tileRect` (page coordinates), in order. `itemsBounds` holds
 * one page-space box (or null) per item. State items are always kept so pushes and pops stay balanced;
 * with `items` given, a null box on a drawing item means "cannot be seen" and it is dropped, without
 * `items` every null box is kept (it might be a state item).
 */
export function visibleItems(itemsBounds, tileRect, items = null) {
  const view = toRect(tileRect)
  const out = []
  for (let i = 0; i < itemsBounds.length; i++) {
    const b = toRect(itemsBounds[i])
    if (b) {
      if (intersects(b, view)) out.push(i)
    } else if (!items || isStateItem(items[i])) {
      out.push(i)
    }
  }
  return out
}

// ---------------------------------------------------------------------------------------------------
// Tiles
// ---------------------------------------------------------------------------------------------------

/** A stable cache key part for a device scale. */
export function scaleKey(scale) {
  return String(Math.round(scale * 1000) / 1000)
}

/**
 * The tiles covering `viewport` (page space) at `zoom` x `dpr`. With `page` ({w, h}) given the tiles
 * are limited to the page and the last row and column are cut to its edge. Each tile:
 * `{key, col, row, scale, rect: {x, y, w, h} in points, px: {x, y, w, h} device pixels from the page origin}`.
 */
export function tilesFor(viewport, zoom, tileSize = 512, dpr = 1, page = null) {
  const v = toRect(viewport)
  const scale = zoom * dpr
  if (!v || !(scale > 0) || !(tileSize > 0) || v.w <= 0 || v.h <= 0) return []
  const side = tileSize / scale
  let x0 = v.x, y0 = v.y, x1 = v.x + v.w, y1 = v.y + v.h
  if (page) {
    x0 = Math.max(0, x0)
    y0 = Math.max(0, y0)
    x1 = Math.min(page.w, x1)
    y1 = Math.min(page.h, y1)
    if (x1 <= x0 || y1 <= y0) return []
  }
  const c0 = Math.floor(x0 / side), r0 = Math.floor(y0 / side)
  const c1 = Math.max(c0, Math.ceil(x1 / side) - 1), r1 = Math.max(r0, Math.ceil(y1 / side) - 1)
  const sk = scaleKey(scale)
  const out = []
  for (let row = r0; row <= r1; row++) {
    for (let col = c0; col <= c1; col++) {
      const x = col * side, y = row * side
      let w = side, h = side
      if (page) {
        w = Math.min(side, page.w - x)
        h = Math.min(side, page.h - y)
      }
      out.push({
        key: `${sk}/${col}/${row}`,
        col,
        row,
        scale,
        rect: { x, y, w, h },
        px: {
          x: col * tileSize,
          y: row * tileSize,
          w: Math.max(1, Math.min(tileSize, Math.ceil(w * scale - 1e-6))),
          h: Math.max(1, Math.min(tileSize, Math.ceil(h * scale - 1e-6))),
        },
      })
    }
  }
  return out
}

/**
 * An LRU of painted tiles (OffscreenCanvas, canvas or ImageBitmap) under a byte budget. The newest
 * entry is never evicted by its own insertion, so a frame can always draw what it just painted.
 * Evicted values are handed to `dispose` (default: `value.close()` when it has one, as ImageBitmap does).
 */
export class TileCache {
  constructor({ budget = 128 * 1024 * 1024, dispose } = {}) {
    this.budget = budget
    this.dispose = dispose || ((v) => v && typeof v.close === 'function' && v.close())
    this.map = new Map() // key -> { value, bytes }; insertion order is the LRU order, oldest first
    this.total = 0
  }

  static bytesOf(value) {
    return value && value.width > 0 && value.height > 0 ? value.width * value.height * 4 : 0
  }

  get size() {
    return this.map.size
  }

  get bytes() {
    return this.total
  }

  has(key) {
    return this.map.has(key)
  }

  /** The value, marked most recently used. */
  get(key) {
    const e = this.map.get(key)
    if (!e) return undefined
    this.map.delete(key)
    this.map.set(key, e)
    return e.value
  }

  /** The value without touching its age. */
  peek(key) {
    return this.map.get(key)?.value
  }

  set(key, value, bytes = TileCache.bytesOf(value)) {
    const old = this.map.get(key)
    if (old) {
      this.map.delete(key)
      this.total -= old.bytes
      if (old.value !== value) this.dispose(old.value)
    }
    this.map.set(key, { value, bytes })
    this.total += bytes
    this.evict(key)
    return value
  }

  /** Drop the oldest entries until the budget holds (never `keep`). */
  evict(keep = null) {
    for (const [k, e] of this.map) {
      if (this.total <= this.budget) break
      if (k === keep) continue
      this.map.delete(k)
      this.total -= e.bytes
      this.dispose(e.value)
    }
  }

  delete(key) {
    const e = this.map.get(key)
    if (!e) return false
    this.map.delete(key)
    this.total -= e.bytes
    this.dispose(e.value)
    return true
  }

  /** Delete every entry for which `pred(key, value)` is true. */
  prune(pred) {
    for (const [k, e] of [...this.map]) if (pred(k, e.value)) this.delete(k)
  }

  /** Keys, least recently used first. */
  keys() {
    return [...this.map.keys()]
  }

  clear() {
    for (const e of this.map.values()) this.dispose(e.value)
    this.map.clear()
    this.total = 0
  }
}

// ---------------------------------------------------------------------------------------------------
// Zoom
// ---------------------------------------------------------------------------------------------------

export const ZOOM_MIN = 0.25
export const ZOOM_MAX = 8
export const ZOOM_LEVELS = Object.freeze([0.25, 0.33, 0.5, 0.67, 0.75, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2, 2.5, 3, 4, 5, 6, 8])

export function clampZoom(z) {
  if (!Number.isFinite(z) || z <= 0) return 1
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, z))
}

/** The nearest zoom level (nearest by ratio, so 0.3 snaps to 0.33 rather than 0.25). */
export function snapZoom(z) {
  const c = clampZoom(z)
  let best = ZOOM_LEVELS[0], bestD = Infinity
  for (const l of ZOOM_LEVELS) {
    const d = Math.abs(Math.log(c / l))
    if (d < bestD - 1e-12) {
      best = l
      bestD = d
    }
  }
  return best
}

/** The next level above `z` (a level a hair above z counts as z). */
export function zoomIn(z) {
  const c = clampZoom(z)
  for (const l of ZOOM_LEVELS) if (l > c * 1.0001) return l
  return ZOOM_MAX
}

export function zoomOut(z) {
  const c = clampZoom(z)
  for (let i = ZOOM_LEVELS.length - 1; i >= 0; i--) if (ZOOM_LEVELS[i] < c / 1.0001) return ZOOM_LEVELS[i]
  return ZOOM_MIN
}

/** The zoom at which the page's width fills the view less `margin` css px on each side. */
export function fitWidth(pageW, viewW, margin = 24) {
  if (!(pageW > 0)) return 1
  return clampZoom(Math.max(1, viewW - 2 * margin) / pageW)
}

/** The zoom at which the whole page fits the view less `margin` on each side. */
export function fitPage(pageW, pageH, viewW, viewH, margin = 24) {
  if (!(pageW > 0) || !(pageH > 0)) return 1
  return clampZoom(Math.min(Math.max(1, viewW - 2 * margin) / pageW, Math.max(1, viewH - 2 * margin) / pageH))
}

// ---------------------------------------------------------------------------------------------------
// Layout of one page in a scrolling view (css pixels)
// ---------------------------------------------------------------------------------------------------

/**
 * Where the page sits in a scrolling view: the scrollable content is the page at `zoom` plus `pad` on
 * every side, centred when smaller than the view. Returns `{zoom, left, top, width, height,
 * contentW, contentH}` with left/top the page's css origin relative to the view's top-left.
 */
export function pageLayout({ pageW, pageH, zoom, viewW, viewH, scrollX = 0, scrollY = 0, pad = 24 }) {
  const width = pageW * zoom, height = pageH * zoom
  const contentW = Math.max(viewW, width + 2 * pad)
  const contentH = Math.max(viewH, height + 2 * pad)
  return {
    zoom,
    left: (contentW - width) / 2 - scrollX,
    top: (contentH - height) / 2 - scrollY,
    width,
    height,
    contentW,
    contentH,
  }
}

/** View css pixel -> page point (the inverse of the zoom and scroll). */
export function screenToPage(cx, cy, layout) {
  return { x: (cx - layout.left) / layout.zoom, y: (cy - layout.top) / layout.zoom }
}

/** Page point -> view css pixel. */
export function pageToScreen(px, py, layout) {
  return { x: px * layout.zoom + layout.left, y: py * layout.zoom + layout.top }
}

/** The part of the page (points) that the view shows, or null when none. */
export function visiblePageRect(layout, viewW, viewH, pageW, pageH) {
  const a = screenToPage(0, 0, layout), b = screenToPage(viewW, viewH, layout)
  const r = intersection(rectLtrb(a.x, a.y, b.x, b.y), { x: 0, y: 0, w: pageW, h: pageH })
  return r && r.w > 0 && r.h > 0 ? r : null
}
