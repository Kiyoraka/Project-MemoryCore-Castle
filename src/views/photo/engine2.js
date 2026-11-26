// PHOTO's second engine: curves, levels, HSL, masks and adjustment layers, transforms, gradients and
// shapes, filters, retouching (heal, content-aware fill) and smart selections (GrabCut, sky, colour
// range). Pure functions on the same plain pixels as engine.js: { data: Uint8ClampedArray (RGBA),
// width, height }. Masks are Uint8Array(width * height), 0..255, 255 = selected / shown.
// Every function returns new pixels (or a new mask) and never touches its input.

import { applyAdjust, blendPixel, brushFalloff, clonePixels, hexToRgba, makePixels } from './engine.js'

const clamp = (v, lo, hi) => (v < lo ? lo : v > hi ? hi : v)
const clamp255 = (v) => (v < 0 ? 0 : v > 255 ? 255 : v)
const lumaOf = (r, g, b) => 0.299 * r + 0.587 * g + 0.114 * b

// A small seeded PRNG (mulberry32) so noise and PatchMatch are repeatable.
export function makeRandom(seed = 1) {
  let s = seed >>> 0 || 1
  return () => {
    s = (s + 0x6d2b79f5) >>> 0
    let t = s
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

// out = orig where the mask is 0, result where it is 255, mixed between (alpha included).
function mixByMask(orig, result, mask) {
  if (!mask) return result
  const o = orig.data
  const r = result.data
  for (let j = 0, i = 0; j < mask.length; j++, i += 4) {
    const m = mask[j]
    if (m === 255) continue
    if (m === 0) {
      r[i] = o[i]
      r[i + 1] = o[i + 1]
      r[i + 2] = o[i + 2]
      r[i + 3] = o[i + 3]
      continue
    }
    const t = m / 255
    for (let k = 0; k < 4; k++) r[i + k] = o[i + k] + (r[i + k] - o[i + k]) * t
  }
  return result
}

// Three LUTs (one per channel) through every pixel.
function applyLuts(pixels, lr, lg, lb, mask) {
  const out = clonePixels(pixels)
  const d = out.data
  for (let i = 0; i < d.length; i += 4) {
    d[i] = lr[d[i]]
    d[i + 1] = lg[d[i + 1]]
    d[i + 2] = lb[d[i + 2]]
  }
  return mixByMask(pixels, out, mask)
}

// ---------- 1. tone and colour ----------

const IDENTITY = Uint8Array.from({ length: 256 }, (_, i) => i)

// A monotone cubic (Fritsch-Carlson) through [x, y] points in 0..255; flat beyond the end points.
export function curvesLut(points) {
  const pts = (Array.isArray(points) ? points : [])
    .map((p) => [clamp(Number(p[0]) || 0, 0, 255), clamp(Number(p[1]) || 0, 0, 255)])
    .sort((a, b) => a[0] - b[0])
    .filter((p, i, all) => i === 0 || p[0] > all[i - 1][0])
  if (pts.length === 0) return new Uint8Array(IDENTITY)
  const lut = new Uint8Array(256)
  if (pts.length === 1) return lut.fill(Math.round(pts[0][1]))
  const n = pts.length
  const xs = pts.map((p) => p[0])
  const ys = pts.map((p) => p[1])
  const delta = []
  for (let i = 0; i < n - 1; i++) delta.push((ys[i + 1] - ys[i]) / (xs[i + 1] - xs[i]))
  const m = new Array(n)
  m[0] = delta[0]
  m[n - 1] = delta[n - 2]
  for (let i = 1; i < n - 1; i++) m[i] = delta[i - 1] * delta[i] <= 0 ? 0 : (delta[i - 1] + delta[i]) / 2
  // Fritsch-Carlson: keep each segment monotone
  for (let i = 0; i < n - 1; i++) {
    if (delta[i] === 0) {
      m[i] = 0
      m[i + 1] = 0
      continue
    }
    const a = m[i] / delta[i]
    const b = m[i + 1] / delta[i]
    const s = a * a + b * b
    if (s > 9) {
      const t = 3 / Math.sqrt(s)
      m[i] = t * a * delta[i]
      m[i + 1] = t * b * delta[i]
    }
  }
  let seg = 0
  for (let x = 0; x < 256; x++) {
    let y
    if (x <= xs[0]) y = ys[0]
    else if (x >= xs[n - 1]) y = ys[n - 1]
    else {
      while (x > xs[seg + 1]) seg++
      const h = xs[seg + 1] - xs[seg]
      const t = (x - xs[seg]) / h
      const t2 = t * t
      const t3 = t2 * t
      y =
        (2 * t3 - 3 * t2 + 1) * ys[seg] +
        (t3 - 2 * t2 + t) * h * m[seg] +
        (-2 * t3 + 3 * t2) * ys[seg + 1] +
        (t3 - t2) * h * m[seg + 1]
    }
    lut[x] = Math.round(clamp255(y))
  }
  return lut
}

// Per-channel curves run first, then the master rgb curve. Each is a list of points or a LUT.
export function applyCurves(pixels, curves = {}, mask = null) {
  const lutOf = (c) => (c instanceof Uint8Array && c.length === 256 ? c : c ? curvesLut(c) : IDENTITY)
  const all = lutOf(curves.rgb)
  const chain = (c) => {
    const l = lutOf(c)
    return Uint8Array.from(l, (v) => all[v])
  }
  return applyLuts(pixels, chain(curves.r), chain(curves.g), chain(curves.b), mask)
}

// Levels: black and white input points, gamma > 1 lifts the midtones; optional output range.
export function levelsLut({ black = 0, gamma = 1, white = 255, outBlack = 0, outWhite = 255 } = {}) {
  const lo = clamp(black, 0, 254)
  const hi = clamp(white, lo + 1, 255)
  const g = clamp(Number(gamma) || 1, 0.01, 10)
  const lut = new Uint8Array(256)
  for (let x = 0; x < 256; x++) {
    const t = Math.pow(clamp((x - lo) / (hi - lo), 0, 1), 1 / g)
    lut[x] = Math.round(clamp255(outBlack + (outWhite - outBlack) * t))
  }
  return lut
}

export function applyLevels(pixels, params = {}, mask = null) {
  const lut = levelsLut(params)
  return applyLuts(pixels, lut, lut, lut, mask)
}

export const HSL_RANGES = { reds: 0, yellows: 60, greens: 120, cyans: 180, blues: 240, magentas: 300 }

export function rgbToHsl(r, g, b) {
  r /= 255
  g /= 255
  b /= 255
  const max = Math.max(r, g, b)
  const min = Math.min(r, g, b)
  const l = (max + min) / 2
  if (max === min) return [0, 0, l]
  const d = max - min
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min)
  let h
  if (max === r) h = (g - b) / d + (g < b ? 6 : 0)
  else if (max === g) h = (b - r) / d + 2
  else h = (r - g) / d + 4
  return [h * 60, s, l]
}

export function hslToRgb(h, s, l) {
  h = (((h % 360) + 360) % 360) / 360
  if (s === 0) return [l * 255, l * 255, l * 255]
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s
  const p = 2 * l - q
  const f = (t) => {
    if (t < 0) t += 1
    if (t > 1) t -= 1
    if (t < 1 / 6) return p + (q - p) * 6 * t
    if (t < 1 / 2) return q
    if (t < 2 / 3) return p + (q - p) * (2 / 3 - t) * 6
    return p
  }
  return [f(h + 1 / 3) * 255, f(h) * 255, f(h - 1 / 3) * 255]
}

// Hue / saturation / lightness for all colours or one range. Ranges overlap softly: a range's
// weight falls linearly to 0 at 60 degrees from its centre, and greys belong to no colour range.
export function applyHsl(pixels, { ranges = {} } = {}, mask = null) {
  const out = clonePixels(pixels)
  const d = out.data
  const read = (r) => ({ hue: Number(r?.hue) || 0, sat: Number(r?.sat) || 0, light: Number(r?.light) || 0 })
  const all = read(ranges.all)
  const named = Object.entries(HSL_RANGES)
    .filter(([k]) => ranges[k])
    .map(([k, centre]) => ({ centre, ...read(ranges[k]) }))
    .filter((r) => r.hue || r.sat || r.light)
  if (!all.hue && !all.sat && !all.light && !named.length) return out
  for (let i = 0; i < d.length; i += 4) {
    const r = d[i]
    const g = d[i + 1]
    const b = d[i + 2]
    let [h, s, l] = rgbToHsl(r, g, b)
    let dh = all.hue
    let ds = all.sat
    let dl = all.light
    if (named.length) {
      const chroma = (Math.max(r, g, b) - Math.min(r, g, b)) / 255
      const colourful = Math.min(1, chroma * 5)
      for (const rg of named) {
        let diff = Math.abs(h - rg.centre) % 360
        if (diff > 180) diff = 360 - diff
        const w = Math.max(0, 1 - diff / 60) * colourful
        if (!w) continue
        dh += rg.hue * w
        ds += rg.sat * w
        dl += rg.light * w
      }
    }
    if (!dh && !ds && !dl) continue
    h += dh
    s = clamp(s * (1 + clamp(ds, -100, 100) / 100), 0, 1)
    const lt = clamp(dl, -100, 100) / 100
    l = lt > 0 ? l + (1 - l) * lt : l * (1 + lt)
    const rgb = hslToRgb(h, s, l)
    d[i] = rgb[0]
    d[i + 1] = rgb[1]
    d[i + 2] = rgb[2]
  }
  return mixByMask(pixels, out, mask)
}

// Counts per value for each channel and for luma; fully transparent pixels are left out.
export function histogram(pixels) {
  const r = new Uint32Array(256)
  const g = new Uint32Array(256)
  const b = new Uint32Array(256)
  const luma = new Uint32Array(256)
  const d = pixels.data
  for (let i = 0; i < d.length; i += 4) {
    if (!d[i + 3]) continue
    r[d[i]]++
    g[d[i + 1]]++
    b[d[i + 2]]++
    luma[Math.round(lumaOf(d[i], d[i + 1], d[i + 2]))]++
  }
  return { r, g, b, luma }
}

// ---------- 2. masks and layers ----------

export function applyMask(pixels, mask) {
  const out = clonePixels(pixels)
  if (!mask) return out
  const d = out.data
  for (let j = 0; j < mask.length; j++) d[j * 4 + 3] = (d[j * 4 + 3] * mask[j]) / 255
  return out
}

export function invertMask(mask) {
  const out = new Uint8Array(mask.length)
  for (let j = 0; j < mask.length; j++) out[j] = 255 - mask[j]
  return out
}

// op: 'add' (union), 'subtract' (a minus b), 'intersect', 'xor'.
export function combineMasks(a, b, op = 'add') {
  const out = new Uint8Array(a.length)
  for (let j = 0; j < a.length; j++) {
    const x = a[j]
    const y = b[j]
    if (op === 'subtract') out[j] = Math.min(x, 255 - y)
    else if (op === 'intersect') out[j] = Math.min(x, y)
    else if (op === 'xor') out[j] = Math.abs(x - y)
    else out[j] = Math.max(x, y)
  }
  return out
}

// One adjustment ({ type, params }) on pixels: 'curves', 'levels', 'hsl' or 'adjust' (engine.js).
export function applyAdjustment(pixels, adjustment) {
  const params = adjustment?.params || {}
  switch (adjustment?.type) {
    case 'curves':
      return applyCurves(pixels, params)
    case 'levels':
      return applyLevels(pixels, params)
    case 'hsl':
      return applyHsl(pixels, params)
    case 'adjust':
      return applyAdjust(pixels, params)
    default:
      return clonePixels(pixels)
  }
}

// Layers bottom-to-top onto a transparent width x height result. Image and text layers carry
// pixels (placed at their optional x, y) and blend through their mask; an adjustment layer changes
// everything already composed below it, mixed by its mask and opacity. Masks are document-sized.
export function compose2(layers, width, height) {
  let out = makePixels(width, height)
  const src = [0, 0, 0, 0]
  const dst = [0, 0, 0, 0]
  for (const layer of layers || []) {
    if (!layer || layer.visible === false) continue
    const opacity = clamp(layer.opacity ?? 1, 0, 1)
    if (!opacity) continue
    const mask = layer.mask || null
    if (layer.kind === 'adjustment') {
      const adjusted = applyAdjustment(out, layer.adjustment)
      const o = out.data
      const a = adjusted.data
      for (let j = 0, i = 0; j < width * height; j++, i += 4) {
        const t = opacity * (mask ? mask[j] / 255 : 1)
        if (!t) continue
        o[i] = o[i] + (a[i] - o[i]) * t
        o[i + 1] = o[i + 1] + (a[i + 1] - o[i + 1]) * t
        o[i + 2] = o[i + 2] + (a[i + 2] - o[i + 2]) * t
      }
      continue
    }
    const p = layer.pixels
    if (!p) continue
    const lx = Math.round(layer.x || 0)
    const ly = Math.round(layer.y || 0)
    const mode = layer.blend || 'normal'
    const o = out.data
    const s = p.data
    const y0 = Math.max(0, ly)
    const y1 = Math.min(height, ly + p.height)
    const x0 = Math.max(0, lx)
    const x1 = Math.min(width, lx + p.width)
    for (let y = y0; y < y1; y++) {
      for (let x = x0; x < x1; x++) {
        const j = y * width + x
        const si = ((y - ly) * p.width + (x - lx)) * 4
        const m = mask ? mask[j] : 255
        if (!m || !s[si + 3]) continue
        const i = j * 4
        src[0] = s[si]
        src[1] = s[si + 1]
        src[2] = s[si + 2]
        src[3] = s[si + 3]
        dst[0] = o[i]
        dst[1] = o[i + 1]
        dst[2] = o[i + 2]
        dst[3] = o[i + 3]
        const r = blendPixel(mode, src, dst, (opacity * m) / 255)
        o[i] = r[0]
        o[i + 1] = r[1]
        o[i + 2] = r[2]
        o[i + 3] = r[3]
      }
    }
  }
  return out
}

// ---------- 3. geometry ----------

// The canvas matrix (x' = a x + c y + e, y' = b x + d y + f) for: scale about (cx, cy), then rotate
// about it (degrees, clockwise on screen), then move by (dx, dy). scale may be a number or [sx, sy].
export function matrixFor({ scale = 1, rotate = 0, dx = 0, dy = 0, cx = 0, cy = 0 } = {}) {
  const sx = Array.isArray(scale) ? scale[0] : scale
  const sy = Array.isArray(scale) ? scale[1] : scale
  const rad = (rotate * Math.PI) / 180
  let cos = Math.cos(rad)
  let sin = Math.sin(rad)
  // snap the right angles so 90-degree turns stay exact
  if (Math.abs(cos) < 1e-12) cos = 0
  if (Math.abs(sin) < 1e-12) sin = 0
  const a = cos * sx
  const b = sin * sx
  const c = -sin * sy
  const d = cos * sy
  return { a, b, c, d, e: cx - a * cx - c * cy + dx, f: cy - b * cx - d * cy + dy }
}

export function invertMatrix({ a, b, c, d, e, f }) {
  const det = a * d - b * c
  if (!det) return null
  return { a: d / det, b: -b / det, c: -c / det, d: a / det, e: (c * f - d * e) / det, f: (b * e - a * f) / det }
}

// Affine transform into an outW x outH picture: each output pixel centre is mapped back into the
// source and sampled bilinearly (premultiplied, so edges fade to transparent without dark fringes).
export function transformPixels(pixels, matrix, outW, outH) {
  const W = Math.max(1, Math.round(outW ?? pixels.width))
  const H = Math.max(1, Math.round(outH ?? pixels.height))
  const out = makePixels(W, H)
  const inv = invertMatrix(matrix)
  if (!inv) return out
  const { width: sw, height: sh, data: s } = pixels
  const o = out.data
  const acc = [0, 0, 0, 0]
  const take = (x, y, w) => {
    if (x < 0 || y < 0 || x >= sw || y >= sh || w <= 0) return
    const i = (y * sw + x) * 4
    const a = s[i + 3] * w
    acc[0] += s[i] * a
    acc[1] += s[i + 1] * a
    acc[2] += s[i + 2] * a
    acc[3] += a
  }
  for (let Y = 0; Y < H; Y++) {
    for (let X = 0; X < W; X++) {
      const px = X + 0.5
      const py = Y + 0.5
      const fx = inv.a * px + inv.c * py + inv.e - 0.5
      const fy = inv.b * px + inv.d * py + inv.f - 0.5
      if (fx <= -1 || fy <= -1 || fx >= sw || fy >= sh) continue
      const x0 = Math.floor(fx)
      const y0 = Math.floor(fy)
      const tx = fx - x0
      const ty = fy - y0
      acc[0] = acc[1] = acc[2] = acc[3] = 0
      take(x0, y0, (1 - tx) * (1 - ty))
      take(x0 + 1, y0, tx * (1 - ty))
      take(x0, y0 + 1, (1 - tx) * ty)
      take(x0 + 1, y0 + 1, tx * ty)
      if (acc[3] <= 0) continue
      const i = (Y * W + X) * 4
      o[i] = acc[0] / acc[3]
      o[i + 1] = acc[1] / acc[3]
      o[i + 2] = acc[2] / acc[3]
      o[i + 3] = acc[3]
    }
  }
  return out
}

// ---------- 4. drawing ----------

// Source-over of one colour (0-255 rgb) with coverage alpha a (0-1) at byte index i.
function over(d, i, r, g, b, a) {
  if (a <= 0) return
  const ab = d[i + 3] / 255
  const ao = a + ab * (1 - a)
  d[i] = (r * a + d[i] * ab * (1 - a)) / ao
  d[i + 1] = (g * a + d[i + 1] * ab * (1 - a)) / ao
  d[i + 2] = (b * a + d[i + 2] * ab * (1 - a)) / ao
  d[i + 3] = ao * 255
}

// A linear or radial gradient through colour stops, laid over the pixels (within the mask).
export function drawGradient(pixels, { kind = 'linear', from = [0, 0], to = [1, 0], stops = [] } = {}, mask = null) {
  const out = clonePixels(pixels)
  const list = (stops.length ? stops : [{ at: 0, color: '#000000' }, { at: 1, color: '#ffffff' }])
    .map((s) => ({ at: clamp(Number(s.at) || 0, 0, 1), rgb: hexToRgba(s.color), alpha: clamp(s.alpha ?? 1, 0, 1) }))
    .sort((a, b) => a.at - b.at)
  const vx = to[0] - from[0]
  const vy = to[1] - from[1]
  const len2 = vx * vx + vy * vy || 1
  const len = Math.sqrt(len2)
  const { width: w, height: h } = out
  const d = out.data
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const px = x + 0.5 - from[0]
      const py = y + 0.5 - from[1]
      let t = kind === 'radial' ? Math.sqrt(px * px + py * py) / len : (px * vx + py * vy) / len2
      t = clamp(t, 0, 1)
      let k = 0
      while (k < list.length - 1 && t > list[k + 1].at) k++
      const s0 = list[k]
      const s1 = list[Math.min(k + 1, list.length - 1)]
      const u = s1.at > s0.at ? clamp((t - s0.at) / (s1.at - s0.at), 0, 1) : t <= s0.at ? 0 : 1
      const pick = t <= list[0].at ? list[0] : t >= list[list.length - 1].at ? list[list.length - 1] : null
      const r = pick ? pick.rgb[0] : s0.rgb[0] + (s1.rgb[0] - s0.rgb[0]) * u
      const g = pick ? pick.rgb[1] : s0.rgb[1] + (s1.rgb[1] - s0.rgb[1]) * u
      const b = pick ? pick.rgb[2] : s0.rgb[2] + (s1.rgb[2] - s0.rgb[2]) * u
      let a = pick ? pick.alpha : s0.alpha + (s1.alpha - s0.alpha) * u
      const j = y * w + x
      if (mask) a *= mask[j] / 255
      over(d, j * 4, r, g, b, a)
    }
  }
  return out
}

function segDist(px, py, ax, ay, bx, by) {
  const vx = bx - ax
  const vy = by - ay
  const l2 = vx * vx + vy * vy
  const t = l2 ? clamp(((px - ax) * vx + (py - ay) * vy) / l2, 0, 1) : 0
  return Math.hypot(px - ax - t * vx, py - ay - t * vy)
}

function insidePoly(x, y, pts) {
  let inside = false
  for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) {
    const [xi, yi] = pts[i]
    const [xj, yj] = pts[j]
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside
  }
  return inside
}

// A rectangle, ellipse, line or polygon with fill and/or stroke, anti-aliased by 4x4 supersampling.
// rect / ellipse use x, y, w, h; line and polygon use points ([[x, y], ...]); a line with no points
// runs from (x, y) to (x + w, y + h).
export function drawShape(pixels, shape = {}, mask = null) {
  const { kind = 'rect', x = 0, y = 0, w = 0, h = 0, fill = null, stroke = null } = shape
  const lw = Math.max(0, Number(shape.width ?? 1))
  const out = clonePixels(pixels)
  const half = lw / 2
  let pts = Array.isArray(shape.points) ? shape.points : []
  if (kind === 'rect') pts = [[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
  if (kind === 'line' && pts.length < 2) pts = [[x, y], [x + w, y + h]]
  const cx = x + w / 2
  const cy = y + h / 2
  const rx = Math.abs(w / 2) || 1e-6
  const ry = Math.abs(h / 2) || 1e-6
  const closed = kind !== 'line'
  const canFill = fill && kind !== 'line'
  const doStroke = stroke && lw > 0
  if (!canFill && !doStroke) return out
  // bounds
  let bx0
  let by0
  let bx1
  let by1
  if (kind === 'ellipse') {
    bx0 = cx - rx
    bx1 = cx + rx
    by0 = cy - ry
    by1 = cy + ry
  } else {
    if (!pts.length) return out
    bx0 = Math.min(...pts.map((p) => p[0]))
    bx1 = Math.max(...pts.map((p) => p[0]))
    by0 = Math.min(...pts.map((p) => p[1]))
    by1 = Math.max(...pts.map((p) => p[1]))
  }
  const pad = doStroke ? half + 1 : 1
  const X0 = clamp(Math.floor(bx0 - pad), 0, out.width)
  const X1 = clamp(Math.ceil(bx1 + pad), 0, out.width)
  const Y0 = clamp(Math.floor(by0 - pad), 0, out.height)
  const Y1 = clamp(Math.ceil(by1 + pad), 0, out.height)
  const inFill = (px, py) => {
    if (kind === 'ellipse') {
      const u = (px - cx) / rx
      const v = (py - cy) / ry
      return u * u + v * v <= 1
    }
    return pts.length >= 3 && insidePoly(px, py, pts)
  }
  const edgeDist = (px, py) => {
    if (kind === 'ellipse') {
      const u = px - cx
      const v = py - cy
      const f = (u * u) / (rx * rx) + (v * v) / (ry * ry) - 1
      const gx = (2 * u) / (rx * rx)
      const gy = (2 * v) / (ry * ry)
      const gl = Math.hypot(gx, gy)
      return gl ? Math.abs(f) / gl : Math.min(rx, ry)
    }
    let best = Infinity
    const n = pts.length
    const last = closed ? n : n - 1
    for (let i = 0; i < last; i++) {
      const a = pts[i]
      const b = pts[(i + 1) % n]
      const dd = segDist(px, py, a[0], a[1], b[0], b[1])
      if (dd < best) best = dd
    }
    return best
  }
  const fc = canFill ? hexToRgba(fill) : null
  const sc = doStroke ? hexToRgba(stroke) : null
  const S = 4
  const d = out.data
  for (let py = Y0; py < Y1; py++) {
    for (let px = X0; px < X1; px++) {
      let nf = 0
      let ns = 0
      for (let sy = 0; sy < S; sy++) {
        for (let sx = 0; sx < S; sx++) {
          const qx = px + (sx + 0.5) / S
          const qy = py + (sy + 0.5) / S
          if (canFill && inFill(qx, qy)) nf++
          if (doStroke && edgeDist(qx, qy) <= half) ns++
        }
      }
      if (!nf && !ns) continue
      const j = py * out.width + px
      const m = mask ? mask[j] / 255 : 1
      if (!m) continue
      if (nf) over(d, j * 4, fc[0], fc[1], fc[2], (nf / (S * S)) * m)
      if (ns) over(d, j * 4, sc[0], sc[1], sc[2], (ns / (S * S)) * m)
    }
  }
  return out
}

// ---------- 5. filters ----------

function gaussKernel(sigma) {
  const r = Math.max(1, Math.ceil(sigma * 3))
  const k = new Float32Array(r * 2 + 1)
  let sum = 0
  for (let i = -r; i <= r; i++) {
    const v = Math.exp(-(i * i) / (2 * sigma * sigma))
    k[i + r] = v
    sum += v
  }
  for (let i = 0; i < k.length; i++) k[i] /= sum
  return { k, r }
}

// Separable gaussian over `ch` interleaved float channels, edges clamped.
function blurFloat(src, w, h, ch, sigma) {
  if (!(sigma > 0)) return src.slice()
  const { k, r } = gaussKernel(sigma)
  const tmp = new Float32Array(src.length)
  const out = new Float32Array(src.length)
  for (let y = 0; y < h; y++) {
    const row = y * w
    for (let x = 0; x < w; x++) {
      for (let c = 0; c < ch; c++) {
        let s = 0
        for (let t = -r; t <= r; t++) {
          const xx = x + t < 0 ? 0 : x + t >= w ? w - 1 : x + t
          s += src[(row + xx) * ch + c] * k[t + r]
        }
        tmp[(row + x) * ch + c] = s
      }
    }
  }
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      for (let c = 0; c < ch; c++) {
        let s = 0
        for (let t = -r; t <= r; t++) {
          const yy = y + t < 0 ? 0 : y + t >= h ? h - 1 : y + t
          s += tmp[(yy * w + x) * ch + c] * k[t + r]
        }
        out[(y * w + x) * ch + c] = s
      }
    }
  }
  return out
}

// Gaussian blur; radius is the gaussian's sigma in pixels. Works premultiplied so transparent
// pixels do not bleed dark colour into their neighbours.
export function gaussianBlur(pixels, radius, mask = null) {
  if (!(radius > 0)) return clonePixels(pixels)
  const { width: w, height: h, data: s } = pixels
  const f = new Float32Array(s.length)
  for (let i = 0; i < s.length; i += 4) {
    const a = s[i + 3] / 255
    f[i] = s[i] * a
    f[i + 1] = s[i + 1] * a
    f[i + 2] = s[i + 2] * a
    f[i + 3] = s[i + 3]
  }
  const b = blurFloat(f, w, h, 4, radius)
  const out = makePixels(w, h)
  const d = out.data
  for (let i = 0; i < d.length; i += 4) {
    const a = b[i + 3]
    d[i + 3] = a
    if (a > 0.001) {
      const u = 255 / a
      d[i] = b[i] * u
      d[i + 1] = b[i + 1] * u
      d[i + 2] = b[i + 2] * u
    }
  }
  return mixByMask(pixels, out, mask)
}

// Unsharp mask: amount in percent (100 = add the full detail once), radius = blur sigma, threshold
// in levels below which a difference is left alone (protects smooth skin and skies).
export function unsharp(pixels, { amount = 100, radius = 1, threshold = 0 } = {}, mask = null) {
  const blurred = gaussianBlur(pixels, radius)
  const out = clonePixels(pixels)
  const d = out.data
  const b = blurred.data
  const k = amount / 100
  for (let i = 0; i < d.length; i += 4) {
    for (let c = 0; c < 3; c++) {
      const diff = pixels.data[i + c] - b[i + c]
      if (Math.abs(diff) < threshold) continue
      d[i + c] = pixels.data[i + c] + diff * k
    }
  }
  return mixByMask(pixels, out, mask)
}

// Gaussian noise: amount 0..100 (percent of half the range as its spread). Same seed, same noise.
export function addNoise(pixels, amount, seed = 1, mask = null, mono = false) {
  const out = clonePixels(pixels)
  if (!amount) return out
  const rand = makeRandom(seed)
  const sd = (clamp(amount, 0, 100) / 100) * 128
  const gauss = () => {
    const u = rand() || 1e-12
    const v = rand()
    return Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * v)
  }
  const d = out.data
  for (let i = 0; i < d.length; i += 4) {
    if (mono) {
      const n = gauss() * sd
      d[i] += n
      d[i + 1] += n
      d[i + 2] += n
    } else {
      d[i] += gauss() * sd
      d[i + 1] += gauss() * sd
      d[i + 2] += gauss() * sd
    }
  }
  return mixByMask(pixels, out, mask)
}

// Vignette: amount -100..100 (positive darkens the edges, negative lightens them); size 0..100 is
// how far from the centre the falloff starts, as a share of the half-diagonal.
export function vignette(pixels, { amount = 50, size = 50 } = {}, mask = null) {
  const out = clonePixels(pixels)
  if (!amount) return out
  const { width: w, height: h } = out
  const d = out.data
  const cx = w / 2
  const cy = h / 2
  const half = Math.hypot(cx, cy) || 1
  const start = clamp(size, 0, 100) / 100
  const amt = clamp(amount, -100, 100) / 100
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const r = Math.hypot(x + 0.5 - cx, y + 0.5 - cy) / half
      let t = start >= 1 ? 0 : clamp((r - start) / (1 - start), 0, 1)
      t = t * t * (3 - 2 * t)
      if (!t) continue
      const i = (y * w + x) * 4
      for (let c = 0; c < 3; c++) {
        d[i + c] = amt > 0 ? d[i + c] * (1 - amt * t) : d[i + c] + (255 - d[i + c]) * -amt * t
      }
    }
  }
  return mixByMask(pixels, out, mask)
}

// Median over a (2r+1) square per colour channel (alpha kept): removes salt-and-pepper specks.
export function median(pixels, radius = 1, mask = null) {
  const r = Math.max(0, Math.round(radius))
  const out = clonePixels(pixels)
  if (!r) return out
  const { width: w, height: h, data: s } = pixels
  const d = out.data
  const size = (2 * r + 1) * (2 * r + 1)
  const half = size >> 1
  const hist = [new Uint16Array(256), new Uint16Array(256), new Uint16Array(256)]
  const addPx = (x, y, sign) => {
    const xx = x < 0 ? 0 : x >= w ? w - 1 : x
    const yy = y < 0 ? 0 : y >= h ? h - 1 : y
    const i = (yy * w + xx) * 4
    hist[0][s[i]] += sign
    hist[1][s[i + 1]] += sign
    hist[2][s[i + 2]] += sign
  }
  for (let y = 0; y < h; y++) {
    hist[0].fill(0)
    hist[1].fill(0)
    hist[2].fill(0)
    for (let dy = -r; dy <= r; dy++) for (let dx = -r; dx <= r; dx++) addPx(dx, y + dy, 1)
    for (let x = 0; x < w; x++) {
      if (x > 0) {
        for (let dy = -r; dy <= r; dy++) {
          addPx(x - r - 1, y + dy, -1)
          addPx(x + r, y + dy, 1)
        }
      }
      const i = (y * w + x) * 4
      for (let c = 0; c < 3; c++) {
        const hc = hist[c]
        let acc = 0
        let v = 0
        while (v < 255 && acc + hc[v] <= half) acc += hc[v++]
        d[i + c] = v
      }
    }
  }
  return mixByMask(pixels, out, mask)
}

// ---------- 6. retouch ----------

// Healing brush: copies from source (offset from the first point) along the stroke, then shifts the
// copy's mean and spread per channel to match a ring of pixels around the destination.
export function heal(pixels, { source, points = [], size = 20, hardness = 0.5 } = {}, mask = null) {
  const out = clonePixels(pixels)
  if (!source || !points.length) return out
  const { width: w, height: h, data: s } = pixels
  const dx = Math.round(source[0] - points[0][0])
  const dy = Math.round(source[1] - points[0][1])
  const R = Math.max(1, size / 2)
  const ring = Math.max(2, Math.round(R / 2))
  const RR = R + ring
  const xs = points.map((p) => p[0])
  const ys = points.map((p) => p[1])
  const X0 = clamp(Math.floor(Math.min(...xs) - RR), 0, w)
  const X1 = clamp(Math.ceil(Math.max(...xs) + RR), 0, w)
  const Y0 = clamp(Math.floor(Math.min(...ys) - RR), 0, h)
  const Y1 = clamp(Math.ceil(Math.max(...ys) + RR), 0, h)
  const bw = X1 - X0
  const bh = Y1 - Y0
  if (bw <= 0 || bh <= 0) return out
  const dist = new Float32Array(bw * bh).fill(Infinity)
  for (const [px, py] of points) {
    const x0 = clamp(Math.floor(px - RR), X0, X1)
    const x1 = clamp(Math.ceil(px + RR), X0, X1)
    const y0 = clamp(Math.floor(py - RR), Y0, Y1)
    const y1 = clamp(Math.ceil(py + RR), Y0, Y1)
    for (let y = y0; y < y1; y++) {
      for (let x = x0; x < x1; x++) {
        const dd = Math.hypot(x + 0.5 - px, y + 0.5 - py)
        const k = (y - Y0) * bw + (x - X0)
        if (dd < dist[k]) dist[k] = dd
      }
    }
  }
  const inImg = (x, y) => x >= 0 && y >= 0 && x < w && y < h
  // ring statistics: destination ring and the matching ring at the source
  const sumD = [0, 0, 0]
  const sqD = [0, 0, 0]
  const sumS = [0, 0, 0]
  const sqS = [0, 0, 0]
  let n = 0
  for (let y = Y0; y < Y1; y++) {
    for (let x = X0; x < X1; x++) {
      const dd = dist[(y - Y0) * bw + (x - X0)]
      if (dd < R || dd > RR || !inImg(x + dx, y + dy)) continue
      const i = (y * w + x) * 4
      const j = ((y + dy) * w + (x + dx)) * 4
      for (let c = 0; c < 3; c++) {
        sumD[c] += s[i + c]
        sqD[c] += s[i + c] * s[i + c]
        sumS[c] += s[j + c]
        sqS[c] += s[j + c] * s[j + c]
      }
      n++
    }
  }
  const shift = [0, 1, 2].map((c) => {
    if (!n) return { mS: 0, mD: 0, k: 1 }
    const mD = sumD[c] / n
    const mS = sumS[c] / n
    const sdD = Math.sqrt(Math.max(0, sqD[c] / n - mD * mD))
    const sdS = Math.sqrt(Math.max(0, sqS[c] / n - mS * mS))
    const k = sdS > 1 ? clamp(sdD / sdS, 0.5, 2) : 1
    return { mS, mD, k }
  })
  const d = out.data
  for (let y = Y0; y < Y1; y++) {
    for (let x = X0; x < X1; x++) {
      const dd = dist[(y - Y0) * bw + (x - X0)]
      let cover = brushFalloff(dd, R, hardness)
      if (!cover) continue
      const sx = clamp(x + dx, 0, w - 1)
      const sy = clamp(y + dy, 0, h - 1)
      const pj = y * w + x
      if (mask) cover *= mask[pj] / 255
      if (!cover) continue
      const i = pj * 4
      const j = (sy * w + sx) * 4
      for (let c = 0; c < 3; c++) {
        const { mS, mD, k } = shift[c]
        const v = clamp255((s[j + c] - mS) * k + mD)
        d[i + c] = s[i + c] + (v - s[i + c]) * cover
      }
    }
  }
  return out
}

// Box downscale by 2 of colours, using only known pixels; a coarse pixel is a hole if any child is.
function downHalf(img, hole, w, h) {
  const W = Math.max(1, Math.ceil(w / 2))
  const H = Math.max(1, Math.ceil(h / 2))
  const out = new Float32Array(W * H * 4)
  const oh = new Uint8Array(W * H)
  for (let Y = 0; Y < H; Y++) {
    for (let X = 0; X < W; X++) {
      const acc = [0, 0, 0, 0]
      let n = 0
      let anyHole = 0
      for (let yy = 2 * Y; yy < Math.min(h, 2 * Y + 2); yy++) {
        for (let xx = 2 * X; xx < Math.min(w, 2 * X + 2); xx++) {
          const j = yy * w + xx
          if (hole[j]) {
            anyHole = 1
            continue
          }
          for (let c = 0; c < 4; c++) acc[c] += img[j * 4 + c]
          n++
        }
      }
      const J = Y * W + X
      oh[J] = anyHole
      if (n) for (let c = 0; c < 4; c++) out[J * 4 + c] = acc[c] / n
    }
  }
  return { img: out, hole: oh, w: W, h: H }
}

// Fills hole pixels from their known neighbours, ring by ring (the start guess for the coarsest level).
function onionFill(img, hole, w, h) {
  const known = new Uint8Array(w * h)
  for (let j = 0; j < known.length; j++) known[j] = hole[j] ? 0 : 1
  let left = known.reduce((a, v) => a + (v ? 0 : 1), 0)
  if (left === known.length) return
  while (left > 0) {
    const fresh = []
    for (let y = 0; y < h; y++) {
      for (let x = 0; x < w; x++) {
        const j = y * w + x
        if (known[j]) continue
        const acc = [0, 0, 0, 0]
        let n = 0
        for (let yy = Math.max(0, y - 1); yy <= Math.min(h - 1, y + 1); yy++) {
          for (let xx = Math.max(0, x - 1); xx <= Math.min(w - 1, x + 1); xx++) {
            const k = yy * w + xx
            if (!known[k]) continue
            for (let c = 0; c < 4; c++) acc[c] += img[k * 4 + c]
            n++
          }
        }
        if (n) fresh.push([j, acc.map((v) => v / n)])
      }
    }
    if (!fresh.length) break
    for (const [j, v] of fresh) {
      for (let c = 0; c < 4; c++) img[j * 4 + c] = v[c]
      known[j] = 1
      left--
    }
  }
}

// PatchMatch at one level: nnf holds a source pixel index for every hole pixel.
function patchMatchLevel(img, hole, w, h, nnf, valid, validList, half, iterations, rand) {
  const holes = []
  for (let j = 0; j < w * h; j++) if (hole[j]) holes.push(j)
  if (!holes.length || !validList.length) return
  const dist = new Float64Array(w * h)
  const area = (2 * half + 1) * (2 * half + 1)
  const patchDist = (p, q, best) => {
    const px = p % w
    const py = (p - px) / w
    const qx = q % w
    const qy = (q - qx) / w
    let sum = 0
    let n = 0
    for (let oy = -half; oy <= half; oy++) {
      const ty = py + oy
      const sy = qy + oy
      if (ty < 0 || ty >= h || sy < 0 || sy >= h) continue
      for (let ox = -half; ox <= half; ox++) {
        const tx = px + ox
        const sx = qx + ox
        if (tx < 0 || tx >= w || sx < 0 || sx >= w) continue
        const a = (ty * w + tx) * 4
        const b = (sy * w + sx) * 4
        const dr = img[a] - img[b]
        const dg = img[a + 1] - img[b + 1]
        const db = img[a + 2] - img[b + 2]
        sum += dr * dr + dg * dg + db * db
        n++
      }
      if (sum > best * area) break // already worse than the best, whatever is left
    }
    return n ? sum / n : Infinity
  }
  for (const p of holes) dist[p] = patchDist(p, nnf[p], Infinity)
  const tryCand = (p, q) => {
    if (q < 0 || q >= w * h || !valid[q] || q === nnf[p]) return
    const dd = patchDist(p, q, dist[p])
    if (dd < dist[p]) {
      dist[p] = dd
      nnf[p] = q
    }
  }
  const maxR = Math.max(w, h)
  for (let it = 0; it < iterations; it++) {
    const forward = it % 2 === 0
    const step = forward ? 1 : -1
    for (let n = forward ? 0 : holes.length - 1; n >= 0 && n < holes.length; n += step) {
      const p = holes[n]
      const px = p % w
      const py = (p - px) / w
      // propagation from the already visited neighbours
      const nx = px - step
      if (nx >= 0 && nx < w && hole[p - step]) {
        const q = nnf[p - step]
        const qx = (q % w) + step
        if (qx >= 0 && qx < w) tryCand(p, q + step)
      }
      const ny = py - step
      if (ny >= 0 && ny < h && hole[p - step * w]) tryCand(p, nnf[p - step * w] + step * w)
      // random search in a shrinking window
      const q0 = nnf[p]
      const qx0 = q0 % w
      const qy0 = (q0 - qx0) / w
      for (let r = maxR; r >= 1; r = Math.floor(r / 2)) {
        const sx = Math.round(qx0 + (rand() * 2 - 1) * r)
        const sy = Math.round(qy0 + (rand() * 2 - 1) * r)
        if (sx < 0 || sy < 0 || sx >= w || sy >= h) continue
        tryCand(p, sy * w + sx)
      }
    }
    vote(img, hole, w, h, nnf, half)
  }
}

// Every hole pixel becomes the average of what the overlapping matched patches say it should be.
function vote(img, hole, w, h, nnf, half) {
  const next = new Float32Array(img.length)
  const weight = new Float32Array(w * h)
  for (let p = 0; p < w * h; p++) {
    if (!hole[p]) continue
    const px = p % w
    const py = (p - px) / w
    const q = nnf[p]
    const qx = q % w
    const qy = (q - qx) / w
    for (let oy = -half; oy <= half; oy++) {
      const ty = py + oy
      const sy = qy + oy
      if (ty < 0 || ty >= h || sy < 0 || sy >= h) continue
      for (let ox = -half; ox <= half; ox++) {
        const tx = px + ox
        const sx = qx + ox
        if (tx < 0 || tx >= w || sx < 0 || sx >= w) continue
        const t = ty * w + tx
        if (!hole[t]) continue
        const s = (sy * w + sx) * 4
        next[t * 4] += img[s]
        next[t * 4 + 1] += img[s + 1]
        next[t * 4 + 2] += img[s + 2]
        next[t * 4 + 3] += img[s + 3]
        weight[t] += 1
      }
    }
  }
  for (let t = 0; t < w * h; t++) {
    if (!hole[t] || !weight[t]) continue
    for (let c = 0; c < 4; c++) img[t * 4 + c] = next[t * 4 + c] / weight[t]
  }
}

// Valid source centres: the whole patch inside the picture and free of hole pixels.
function validSources(hole, w, h, half) {
  const valid = new Uint8Array(w * h)
  // integral image of the hole for O(1) patch checks
  const I = new Int32Array((w + 1) * (h + 1))
  for (let y = 0; y < h; y++) {
    let row = 0
    for (let x = 0; x < w; x++) {
      row += hole[y * w + x] ? 1 : 0
      I[(y + 1) * (w + 1) + x + 1] = I[y * (w + 1) + x + 1] + row
    }
  }
  const list = []
  for (let y = half; y < h - half; y++) {
    for (let x = half; x < w - half; x++) {
      const x0 = x - half
      const y0 = y - half
      const x1 = x + half + 1
      const y1 = y + half + 1
      const n = I[y1 * (w + 1) + x1] - I[y0 * (w + 1) + x1] - I[y1 * (w + 1) + x0] + I[y0 * (w + 1) + x0]
      if (!n) {
        valid[y * w + x] = 1
        list.push(y * w + x)
      }
    }
  }
  if (!list.length) {
    // the hole is too big for whole patches: any known pixel will do
    for (let j = 0; j < w * h; j++) {
      if (!hole[j]) {
        valid[j] = 1
        list.push(j)
      }
    }
  }
  return { valid, list }
}

// Content-aware fill (PatchMatch): hole pixels (mask > 127) are rebuilt from patches of the rest of
// the picture, coarse to fine over a small pyramid. Pixels outside the hole are never changed.
export function contentAwareFill(pixels, holeMask, { iterations = 5, patch = 7, seed = 1 } = {}) {
  const { width: w, height: h, data } = pixels
  const out = clonePixels(pixels)
  const hole0 = new Uint8Array(w * h)
  let any = false
  for (let j = 0; j < hole0.length; j++) {
    if (holeMask[j] > 127) {
      hole0[j] = 1
      any = true
    }
  }
  if (!any) return out
  const half = Math.max(1, Math.floor(patch / 2))
  const rand = makeRandom(seed)
  // pyramid: up to 3 levels, the coarsest still a few patches wide
  const levels = [{ img: Float32Array.from(data), hole: hole0, w, h }]
  while (levels.length < 3) {
    const top = levels[levels.length - 1]
    if (Math.min(top.w, top.h) / 2 < patch * 4) break
    levels.push(downHalf(top.img, top.hole, top.w, top.h))
  }
  let nnf = null
  let prevW = 0
  for (let L = levels.length - 1; L >= 0; L--) {
    const { img, hole, w: lw, h: lh } = levels[L]
    const { valid, list } = validSources(hole, lw, lh, half)
    if (!list.length) return out
    const next = new Int32Array(lw * lh)
    for (let p = 0; p < lw * lh; p++) {
      if (!hole[p]) {
        next[p] = p
        continue
      }
      let q = -1
      if (nnf) {
        // carry the coarse match up: same offset, doubled
        const px = p % lw
        const py = (p - px) / lw
        const cp = Math.min(Math.floor(py / 2), Math.floor(levels[L + 1].h - 1)) * prevW + Math.min(Math.floor(px / 2), prevW - 1)
        const cq = nnf[cp]
        const cqx = cq % prevW
        const cqy = (cq - cqx) / prevW
        const qx = cqx * 2 + (px % 2)
        const qy = cqy * 2 + (py % 2)
        if (qx < lw && qy < lh && valid[qy * lw + qx]) q = qy * lw + qx
      }
      if (q < 0) q = list[Math.floor(rand() * list.length)]
      next[p] = q
    }
    if (L === levels.length - 1) onionFill(img, hole, lw, lh)
    else vote(img, hole, lw, lh, next, half)
    patchMatchLevel(img, hole, lw, lh, next, valid, list, half, L === levels.length - 1 ? iterations + 2 : iterations, rand)
    nnf = next
    prevW = lw
  }
  const fine = levels[0].img
  const d = out.data
  for (let j = 0; j < w * h; j++) {
    if (!hole0[j]) continue
    for (let c = 0; c < 4; c++) d[j * 4 + c] = fine[j * 4 + c]
  }
  return out
}

// ---------- 7. selection ----------

// Max (grow) or min (shrink) over a square of radius r, separable.
function morph(m, w, h, r, grow) {
  const tmp = new Uint8Array(m.length)
  const out = new Uint8Array(m.length)
  const pick = grow ? Math.max : Math.min
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      let v = m[y * w + x]
      for (let t = Math.max(0, x - r); t <= Math.min(w - 1, x + r); t++) v = pick(v, m[y * w + t])
      tmp[y * w + x] = v
    }
  }
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      let v = tmp[y * w + x]
      for (let t = Math.max(0, y - r); t <= Math.min(h - 1, y + r); t++) v = pick(v, tmp[t * w + x])
      out[y * w + x] = v
    }
  }
  return out
}

function blurMask(mask, w, h, sigma) {
  const f = Float32Array.from(mask)
  const b = blurFloat(f, w, h, 1, sigma)
  return Uint8Array.from(b, (v) => Math.round(clamp255(v)))
}

// Select-and-mask: smooth (px, rounds off jagged edges and drops specks), shift (-20..20 px, grow
// or shrink), then feather (px, soft edge). Order: smooth, shift, feather.
export function refineMask(mask, width, height, { feather = 0, smooth = 0, shift = 0 } = {}) {
  let m = new Uint8Array(mask)
  if (smooth > 0) {
    const b = blurMask(m, width, height, smooth / 2)
    m = Uint8Array.from(b, (v) => (v >= 128 ? 255 : 0))
  }
  const sh = Math.round(clamp(shift, -20, 20))
  if (sh) m = morph(m, width, height, Math.abs(sh), sh > 0)
  if (feather > 0) m = blurMask(m, width, height, feather)
  return m
}

// A soft mask of pixels near one colour: full within fuzz/2, fading to nothing at fuzz.
export function colorRange(pixels, color, fuzz = 40) {
  const [cr, cg, cb] = hexToRgba(color)
  const d = pixels.data
  const out = new Uint8Array(pixels.width * pixels.height)
  const f = Math.max(0, fuzz)
  for (let j = 0, i = 0; j < out.length; j++, i += 4) {
    const dist = Math.sqrt(((d[i] - cr) ** 2 + (d[i + 1] - cg) ** 2 + (d[i + 2] - cb) ** 2) / 3)
    if (f === 0) out[j] = dist === 0 ? 255 : 0
    else if (dist <= f / 2) out[j] = 255
    else out[j] = Math.round(255 * clamp(1 - (dist - f / 2) / (f / 2), 0, 1))
  }
  return out
}

// Sky: seeds from the brightest / bluest pixels in the top 15% band, then grows by colour (near the
// seeds' average) and stops where the step between neighbours is sharp (the horizon, a roof line).
export function skyMask(pixels, { tolerance = 40 } = {}) {
  const { width: w, height: h, data: d } = pixels
  const mask = new Uint8Array(w * h)
  const band = Math.max(1, Math.ceil(h * 0.15))
  const scores = []
  for (let y = 0; y < band; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4
      const r = d[i]
      const g = d[i + 1]
      const b = d[i + 2]
      scores.push({ j: y * w + x, s: lumaOf(r, g, b) + 1.5 * (b - (r + g) / 2) })
    }
  }
  scores.sort((a, b) => b.s - a.s)
  const top = scores.slice(0, Math.max(1, Math.ceil(scores.length * 0.3)))
  const mean = [0, 0, 0]
  for (const { j } of top) for (let c = 0; c < 3; c++) mean[c] += d[j * 4 + c] / top.length
  const tol = Math.max(1, tolerance)
  const near = (j) => {
    const i = j * 4
    return Math.sqrt(((d[i] - mean[0]) ** 2 + (d[i + 1] - mean[1]) ** 2 + (d[i + 2] - mean[2]) ** 2) / 3) <= tol * 1.5
  }
  const step = (a, b) => {
    const i = a * 4
    const k = b * 4
    return Math.max(Math.abs(d[i] - d[k]), Math.abs(d[i + 1] - d[k + 1]), Math.abs(d[i + 2] - d[k + 2]))
  }
  const stack = []
  for (const { j } of top) {
    if (!near(j)) continue
    mask[j] = 255
    stack.push(j)
  }
  const edge = tol / 2
  while (stack.length) {
    const j = stack.pop()
    const x = j % w
    const y = (j - x) / w
    const around = []
    if (x > 0) around.push(j - 1)
    if (x < w - 1) around.push(j + 1)
    if (y > 0) around.push(j - w)
    if (y < h - 1) around.push(j + w)
    for (const n of around) {
      if (mask[n] || step(j, n) > edge || !near(n)) continue
      mask[n] = 255
      stack.push(n)
    }
  }
  const s = Math.max(1, Math.round(Math.min(w, h) / 200))
  return refineMask(mask, w, h, { smooth: s * 2, feather: s })
}

// ---- GrabCut ----

function gmmFit(samples, K, rand) {
  // samples: Float64Array of rgb triples. k-means (k-means++ start) then one Gaussian per cluster.
  const n = samples.length / 3
  const k = Math.max(1, Math.min(K, n))
  const centres = []
  const first = Math.floor(rand() * n)
  centres.push([samples[first * 3], samples[first * 3 + 1], samples[first * 3 + 2]])
  const dmin = new Float64Array(n).fill(Infinity)
  while (centres.length < k) {
    const c = centres[centres.length - 1]
    let total = 0
    for (let i = 0; i < n; i++) {
      const dd = (samples[i * 3] - c[0]) ** 2 + (samples[i * 3 + 1] - c[1]) ** 2 + (samples[i * 3 + 2] - c[2]) ** 2
      if (dd < dmin[i]) dmin[i] = dd
      total += dmin[i]
    }
    if (total <= 0) break
    let pick = rand() * total
    let idx = 0
    for (; idx < n - 1; idx++) {
      pick -= dmin[idx]
      if (pick <= 0) break
    }
    centres.push([samples[idx * 3], samples[idx * 3 + 1], samples[idx * 3 + 2]])
  }
  const label = new Int32Array(n)
  for (let it = 0; it < 6; it++) {
    for (let i = 0; i < n; i++) {
      let best = 0
      let bd = Infinity
      for (let c = 0; c < centres.length; c++) {
        const ce = centres[c]
        const dd = (samples[i * 3] - ce[0]) ** 2 + (samples[i * 3 + 1] - ce[1]) ** 2 + (samples[i * 3 + 2] - ce[2]) ** 2
        if (dd < bd) {
          bd = dd
          best = c
        }
      }
      label[i] = best
    }
    const sums = centres.map(() => [0, 0, 0, 0])
    for (let i = 0; i < n; i++) {
      const s = sums[label[i]]
      s[0] += samples[i * 3]
      s[1] += samples[i * 3 + 1]
      s[2] += samples[i * 3 + 2]
      s[3]++
    }
    sums.forEach((s, c) => {
      if (s[3]) centres[c] = [s[0] / s[3], s[1] / s[3], s[2] / s[3]]
    })
  }
  return gmmFromLabels(samples, label, centres.length)
}

function gmmFromLabels(samples, label, k) {
  const n = samples.length / 3
  const acc = Array.from({ length: k }, () => ({ n: 0, s: [0, 0, 0], ss: [0, 0, 0, 0, 0, 0, 0, 0, 0] }))
  for (let i = 0; i < n; i++) {
    const a = acc[label[i]]
    const v = [samples[i * 3], samples[i * 3 + 1], samples[i * 3 + 2]]
    a.n++
    for (let p = 0; p < 3; p++) {
      a.s[p] += v[p]
      for (let q = 0; q < 3; q++) a.ss[p * 3 + q] += v[p] * v[q]
    }
  }
  const comps = []
  for (const a of acc) {
    if (!a.n) continue
    const m = a.s.map((v) => v / a.n)
    const C = new Array(9)
    for (let p = 0; p < 3; p++) for (let q = 0; q < 3; q++) C[p * 3 + q] = a.ss[p * 3 + q] / a.n - m[p] * m[q]
    for (let p = 0; p < 3; p++) C[p * 4] += 4 // regularise: flat colours stay finite
    const det =
      C[0] * (C[4] * C[8] - C[5] * C[7]) - C[1] * (C[3] * C[8] - C[5] * C[6]) + C[2] * (C[3] * C[7] - C[4] * C[6])
    const inv = [
      (C[4] * C[8] - C[5] * C[7]) / det,
      (C[2] * C[7] - C[1] * C[8]) / det,
      (C[1] * C[5] - C[2] * C[4]) / det,
      (C[5] * C[6] - C[3] * C[8]) / det,
      (C[0] * C[8] - C[2] * C[6]) / det,
      (C[2] * C[3] - C[0] * C[5]) / det,
      (C[3] * C[7] - C[4] * C[6]) / det,
      (C[1] * C[6] - C[0] * C[7]) / det,
      (C[0] * C[4] - C[1] * C[3]) / det,
    ]
    comps.push({ w: a.n / n, m, inv, logNorm: Math.log(a.n / n) - 0.5 * Math.log(det) })
  }
  return comps
}

// Per component log-likelihood (dropping the shared 2*pi constant).
function compLog(c, r, g, b) {
  const x = r - c.m[0]
  const y = g - c.m[1]
  const z = b - c.m[2]
  const I = c.inv
  const q = x * (I[0] * x + I[1] * y + I[2] * z) + y * (I[3] * x + I[4] * y + I[5] * z) + z * (I[6] * x + I[7] * y + I[8] * z)
  return c.logNorm - 0.5 * q
}

function gmmCost(comps, r, g, b) {
  let best = -Infinity
  const logs = comps.map((c) => {
    const v = compLog(c, r, g, b)
    if (v > best) best = v
    return v
  })
  let s = 0
  for (const v of logs) s += Math.exp(v - best)
  return -(best + Math.log(s))
}

function bestComp(comps, r, g, b) {
  let best = 0
  let bv = -Infinity
  comps.forEach((c, k) => {
    const v = compLog(c, r, g, b)
    if (v > bv) {
      bv = v
      best = k
    }
  })
  return best
}

// Dinic max-flow on a graph stored as edge arrays; returns the set reachable from the source.
function minCut(nNodes, edges, s, t) {
  const m = edges.length
  const head = new Int32Array(nNodes).fill(-1)
  const next = new Int32Array(m * 2)
  const to = new Int32Array(m * 2)
  const cap = new Float64Array(m * 2)
  let e = 0
  for (const [u, v, c, rc] of edges) {
    to[e] = v
    cap[e] = c
    next[e] = head[u]
    head[u] = e++
    to[e] = u
    cap[e] = rc
    next[e] = head[v]
    head[v] = e++
  }
  const EPS = 1e-9
  const level = new Int32Array(nNodes)
  const it = new Int32Array(nNodes)
  const queue = new Int32Array(nNodes)
  const bfs = () => {
    level.fill(-1)
    let qh = 0
    let qt = 0
    queue[qt++] = s
    level[s] = 0
    while (qh < qt) {
      const u = queue[qh++]
      for (let x = head[u]; x !== -1; x = next[x]) {
        if (cap[x] > EPS && level[to[x]] < 0) {
          level[to[x]] = level[u] + 1
          queue[qt++] = to[x]
        }
      }
    }
    return level[t] >= 0
  }
  const path = new Int32Array(nNodes)
  while (bfs()) {
    for (let u = 0; u < nNodes; u++) it[u] = head[u]
    let len = 0
    let u = s
    for (;;) {
      if (u === t) {
        let f = Infinity
        for (let i = 0; i < len; i++) if (cap[path[i]] < f) f = cap[path[i]]
        let cut = -1
        for (let i = 0; i < len; i++) {
          cap[path[i]] -= f
          cap[path[i] ^ 1] += f
          if (cut < 0 && cap[path[i]] <= EPS) cut = i
        }
        len = cut
        u = len === 0 ? s : to[path[len - 1]]
        continue
      }
      let x = it[u]
      while (x !== -1 && !(cap[x] > EPS && level[to[x]] === level[u] + 1)) x = next[x]
      it[u] = x
      if (x === -1) {
        level[u] = -1
        if (len === 0) break
        len--
        u = to[path[len] ^ 1]
        it[u] = next[it[u]]
        continue
      }
      path[len++] = x
      u = to[x]
    }
  }
  // what the source still reaches
  const seen = new Uint8Array(nNodes)
  const stack = [s]
  seen[s] = 1
  while (stack.length) {
    const u = stack.pop()
    for (let x = head[u]; x !== -1; x = next[x]) {
      if (cap[x] > EPS && !seen[to[x]]) {
        seen[to[x]] = 1
        stack.push(to[x])
      }
    }
  }
  return seen
}

// GrabCut: rect { x, y, w, h } holds the subject; hints add sure foreground / background points.
// Works on a copy no larger than maxSide, with 5-component colour models per side and a graph cut
// on the 8-neighbour grid; the result is scaled up and its edge re-decided at full size.
export function grabCut(pixels, rect, { hints = {}, iterations = 3, maxSide = 160, seed = 1 } = {}) {
  const { width: W, height: H } = pixels
  const full = new Uint8Array(W * H)
  const scale = Math.min(1, maxSide / Math.max(W, H))
  const w = Math.max(1, Math.round(W * scale))
  const h = Math.max(1, Math.round(H * scale))
  const sx = w / W
  const sy = h / H
  const small = scale < 1 ? boxResize(pixels, w, h) : pixels
  const d = small.data
  const n = w * h
  const rx0 = clamp(Math.floor(rect.x * sx), 0, w)
  const ry0 = clamp(Math.floor(rect.y * sy), 0, h)
  const rx1 = clamp(Math.ceil((rect.x + rect.w) * sx), 0, w)
  const ry1 = clamp(Math.ceil((rect.y + rect.h) * sy), 0, h)
  if (rx1 <= rx0 || ry1 <= ry0) return full
  // 0 sure bg, 1 sure fg, 2 maybe bg, 3 maybe fg
  const state = new Uint8Array(n)
  for (let y = ry0; y < ry1; y++) for (let x = rx0; x < rx1; x++) state[y * w + x] = 3
  const paint = (pts, v) => {
    for (const [px, py] of pts || []) {
      const cx = Math.round(px * sx)
      const cy = Math.round(py * sy)
      for (let y = cy - 1; y <= cy + 1; y++) for (let x = cx - 1; x <= cx + 1; x++) if (x >= 0 && y >= 0 && x < w && y < h) state[y * w + x] = v
    }
  }
  paint(hints.bg, 0)
  paint(hints.fg, 1)
  const isFg = (st) => st === 1 || st === 3
  // smoothness weights
  const GAMMA = 50
  const nb = [
    [1, 0, 1],
    [0, 1, 1],
    [1, 1, Math.SQRT1_2],
    [-1, 1, Math.SQRT1_2],
  ]
  let sumD = 0
  let cntD = 0
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4
      for (const [ox, oy] of nb) {
        const xx = x + ox
        const yy = y + oy
        if (xx < 0 || yy < 0 || xx >= w || yy >= h) continue
        const k = (yy * w + xx) * 4
        sumD += (d[i] - d[k]) ** 2 + (d[i + 1] - d[k + 1]) ** 2 + (d[i + 2] - d[k + 2]) ** 2
        cntD++
      }
    }
  }
  const beta = sumD > 0 ? 1 / (2 * (sumD / cntD)) : 0
  const links = []
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const j = y * w + x
      const i = j * 4
      for (const [ox, oy, dist] of nb) {
        const xx = x + ox
        const yy = y + oy
        if (xx < 0 || yy < 0 || xx >= w || yy >= h) continue
        const k = yy * w + xx
        const diff = (d[i] - d[k * 4]) ** 2 + (d[i + 1] - d[k * 4 + 1]) ** 2 + (d[i + 2] - d[k * 4 + 2]) ** 2
        const wgt = GAMMA * dist * Math.exp(-beta * diff)
        links.push([j, k, wgt, wgt])
      }
    }
  }
  const BIG = GAMMA * 8 * 10 + 1 // more than any cut through a pixel's links
  const rand = makeRandom(seed)
  const collect = (wantFg) => {
    let cnt = 0
    for (let j = 0; j < n; j++) if (isFg(state[j]) === wantFg) cnt++
    const s = new Float64Array(cnt * 3)
    let q = 0
    for (let j = 0; j < n; j++) {
      if (isFg(state[j]) !== wantFg) continue
      s[q++] = d[j * 4]
      s[q++] = d[j * 4 + 1]
      s[q++] = d[j * 4 + 2]
    }
    return s
  }
  let fgModel = null
  let bgModel = null
  const S = n
  const T = n + 1
  for (let it = 0; it < Math.max(1, iterations); it++) {
    const fs = collect(true)
    const bs = collect(false)
    if (!fs.length || !bs.length) break
    if (!fgModel) {
      fgModel = gmmFit(fs, 5, rand)
      bgModel = gmmFit(bs, 5, rand)
    } else {
      // reassign each sample to its likeliest component and refit
      const refit = (samples, model) => {
        const lab = new Int32Array(samples.length / 3)
        for (let i = 0; i < lab.length; i++) lab[i] = bestComp(model, samples[i * 3], samples[i * 3 + 1], samples[i * 3 + 2])
        return gmmFromLabels(samples, lab, model.length)
      }
      fgModel = refit(fs, fgModel)
      bgModel = refit(bs, bgModel)
    }
    const edges = links.slice()
    for (let j = 0; j < n; j++) {
      const st = state[j]
      let toS = 0
      let toT = 0
      if (st === 0) toT = BIG
      else if (st === 1) toS = BIG
      else {
        const r = d[j * 4]
        const g = d[j * 4 + 1]
        const b = d[j * 4 + 2]
        toS = gmmCost(bgModel, r, g, b) // paid if labelled background
        toT = gmmCost(fgModel, r, g, b) // paid if labelled foreground
      }
      const net = toS - toT
      if (net > 0) edges.push([S, j, net, 0])
      else if (net < 0) edges.push([j, T, -net, 0])
    }
    const reach = minCut(n + 2, edges, S, T)
    let changed = 0
    for (let j = 0; j < n; j++) {
      if (state[j] < 2) continue
      const v = reach[j] ? 3 : 2
      if (v !== state[j]) changed++
      state[j] = v
    }
    if (!changed && it > 0) break
  }
  // scale up, then re-decide the edge band at full size with the colour models
  const fx = W / w
  const fy = H / h
  const small8 = Uint8Array.from(state, (st) => (isFg(st) ? 255 : 0))
  const pd = pixels.data
  for (let Y = 0; Y < H; Y++) {
    const gy = clamp((Y + 0.5) / fy - 0.5, 0, h - 1)
    const y0 = Math.floor(gy)
    const y1 = Math.min(h - 1, y0 + 1)
    const ty = gy - y0
    for (let X = 0; X < W; X++) {
      const gx = clamp((X + 0.5) / fx - 0.5, 0, w - 1)
      const x0 = Math.floor(gx)
      const x1 = Math.min(w - 1, x0 + 1)
      const tx = gx - x0
      const top = small8[y0 * w + x0] * (1 - tx) + small8[y0 * w + x1] * tx
      const bot = small8[y1 * w + x0] * (1 - tx) + small8[y1 * w + x1] * tx
      let v = top * (1 - ty) + bot * ty
      const J = Y * W + X
      if (v > 8 && v < 247 && fgModel && bgModel && scale < 1) {
        const i = J * 4
        v = gmmCost(fgModel, pd[i], pd[i + 1], pd[i + 2]) < gmmCost(bgModel, pd[i], pd[i + 1], pd[i + 2]) ? 255 : 0
      }
      full[J] = v >= 128 ? 255 : 0
    }
  }
  // nothing outside the rectangle, unless a foreground hint put it there
  for (let Y = 0; Y < H; Y++) {
    for (let X = 0; X < W; X++) {
      if (X >= rect.x && X < rect.x + rect.w && Y >= rect.y && Y < rect.y + rect.h) continue
      const st = state[Math.min(h - 1, Math.floor((Y + 0.5) * sy)) * w + Math.min(w - 1, Math.floor((X + 0.5) * sx))]
      if (st !== 1 && st !== 3) full[Y * W + X] = 0
    }
  }
  return refineMask(full, W, H, { smooth: scale < 1 ? 1 : 0, feather: 0.6 })
}

// Area-average downscale (for GrabCut's working copy).
function boxResize(p, w, h) {
  const out = makePixels(w, h)
  const { width: W, height: H, data: s } = p
  const o = out.data
  for (let y = 0; y < h; y++) {
    const ya = Math.floor((y * H) / h)
    const yb = Math.max(ya + 1, Math.floor(((y + 1) * H) / h))
    for (let x = 0; x < w; x++) {
      const xa = Math.floor((x * W) / w)
      const xb = Math.max(xa + 1, Math.floor(((x + 1) * W) / w))
      const acc = [0, 0, 0, 0]
      let n = 0
      for (let yy = ya; yy < yb; yy++) {
        for (let xx = xa; xx < xb; xx++) {
          const i = (yy * W + xx) * 4
          acc[0] += s[i]
          acc[1] += s[i + 1]
          acc[2] += s[i + 2]
          acc[3] += s[i + 3]
          n++
        }
      }
      const i = (y * w + x) * 4
      for (let c = 0; c < 4; c++) o[i + c] = acc[c] / n
    }
  }
  return out
}
