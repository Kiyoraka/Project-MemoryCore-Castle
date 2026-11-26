// PHOTO's third engine: the 27 blend modes, layer styles, groups and clipping masks, the rest of
// Photoshop's adjustments and filters, liquify and warps, retouch brushes, quick / object select,
// and snap and artboard helpers. Pure functions on the same plain pixels as engine.js and engine2.js:
// { data: Uint8ClampedArray (RGBA), width, height }. Masks are Uint8Array(width * height), 0..255.
// Every function returns new pixels (or a new mask / mesh) and never touches its input.

import { brushFalloff, clonePixels, cropPixels, hexToRgba, makePixels } from './engine.js'
import { applyAdjustment, applyMask, combineMasks, drawGradient, gaussianBlur, grabCut, heal, makeRandom, refineMask, unsharp } from './engine2.js'

// ---------- shared helpers ----------

const clamp = (v, lo, hi) => (v < lo ? lo : v > hi ? hi : v)
const clamp01 = (v) => (v < 0 ? 0 : v > 1 ? 1 : v)
const clamp255 = (v) => (v < 0 ? 0 : v > 255 ? 255 : v)
const lumaOf = (r, g, b) => 0.299 * r + 0.587 * g + 0.114 * b
const num = (v, d) => (Number.isFinite(Number(v)) && v !== null && v !== '' ? Number(v) : d)
const smooth = (a, b, x) => {
  const t = clamp01((x - a) / (b - a))
  return t * t * (3 - 2 * t)
}
const toPt = (p) => (Array.isArray(p) ? [Number(p[0]) || 0, Number(p[1]) || 0] : [Number(p?.x) || 0, Number(p?.y) || 0])
const rgbOf = (c, fallback = '#000000') => (Array.isArray(c) ? c : hexToRgba(c ?? fallback))

// out = orig where the mask is 0, result where it is 255, mixed between (alpha included).
function mixByMask(orig, result, mask) {
  if (!mask) return result
  const o = orig.data
  const r = result.data
  for (let j = 0, i = 0; j < mask.length; j++, i += 4) {
    const m = mask[j]
    if (m === 255) continue
    const t = m / 255
    for (let k = 0; k < 4; k++) r[i + k] = o[i + k] + (r[i + k] - o[i + k]) * t
  }
  return result
}

// Runs fn(r, g, b, out3, j) on every pixel's colour (alpha kept), then mixes by the mask.
function mapRgb(pixels, mask, fn) {
  const out = clonePixels(pixels)
  const d = out.data
  const o = [0, 0, 0]
  for (let i = 0, j = 0; i < d.length; i += 4, j++) {
    if (mask && !mask[j]) continue
    fn(d[i], d[i + 1], d[i + 2], o, j)
    d[i] = o[0]
    d[i + 1] = o[1]
    d[i + 2] = o[2]
  }
  return mixByMask(pixels, out, mask)
}

function lut3(pixels, lr, lg, lb, mask) {
  return mapRgb(pixels, mask, (r, g, b, o) => {
    o[0] = lr[r]
    o[1] = lg[g]
    o[2] = lb[b]
  })
}

function premul(p) {
  const s = p.data
  const f = new Float32Array(s.length)
  for (let i = 0; i < s.length; i += 4) {
    const a = s[i + 3] / 255
    f[i] = s[i] * a
    f[i + 1] = s[i + 1] * a
    f[i + 2] = s[i + 2] * a
    f[i + 3] = s[i + 3]
  }
  return f
}

function unpremul(f, w, h) {
  const out = makePixels(w, h)
  const d = out.data
  for (let i = 0; i < d.length; i += 4) {
    const a = f[i + 3]
    d[i + 3] = a
    if (a > 0.001) {
      const u = 255 / a
      d[i] = f[i] * u
      d[i + 1] = f[i + 1] * u
      d[i + 2] = f[i + 2] * u
    }
  }
  return out
}

// Bilinear read of premultiplied floats at (fx, fy) in pixel-centre coordinates, adding w times
// the sample into acc. Outside the picture it clamps to the edge, or reads nothing when `clear`.
function samplePremul(f, w, h, fx, fy, acc, wt = 1, clear = false) {
  if (clear && (fx <= -1 || fy <= -1 || fx >= w || fy >= h)) return
  const x0 = Math.floor(fx)
  const y0 = Math.floor(fy)
  const tx = fx - x0
  const ty = fy - y0
  const corner = (x, y, cw) => {
    if (cw <= 0) return
    if (x < 0 || y < 0 || x >= w || y >= h) {
      if (clear) return
      x = clamp(x, 0, w - 1)
      y = clamp(y, 0, h - 1)
    }
    const i = (y * w + x) * 4
    const k = cw * wt
    acc[0] += f[i] * k
    acc[1] += f[i + 1] * k
    acc[2] += f[i + 2] * k
    acc[3] += f[i + 3] * k
  }
  corner(x0, y0, (1 - tx) * (1 - ty))
  corner(x0 + 1, y0, tx * (1 - ty))
  corner(x0, y0 + 1, (1 - tx) * ty)
  corner(x0 + 1, y0 + 1, tx * ty)
}

function writeAcc(d, i, acc, div = 1) {
  const a = acc[3] / div
  d[i + 3] = a
  if (a > 0.001) {
    const u = 255 / (acc[3] || 1)
    d[i] = acc[0] * u
    d[i + 1] = acc[1] * u
    d[i + 2] = acc[2] * u
  }
}

// One box pass (running sum, edges clamped) along rows or columns of a float field.
function boxPass(src, dst, w, h, r, horizontal) {
  if (r < 1) {
    dst.set(src)
    return
  }
  const n = horizontal ? w : h
  const lines = horizontal ? h : w
  const stride = horizontal ? 1 : w
  const lineStep = horizontal ? w : 1
  const inv = 1 / (2 * r + 1)
  for (let l = 0; l < lines; l++) {
    const base = l * lineStep
    let acc = 0
    for (let k = -r; k <= r; k++) acc += src[base + clamp(k, 0, n - 1) * stride]
    for (let i = 0; i < n; i++) {
      dst[base + i * stride] = acc * inv
      acc += src[base + Math.min(n - 1, i + r + 1) * stride] - src[base + Math.max(0, i - r) * stride]
    }
  }
}

// Three box blurs approximate a gaussian of this sigma, in time that does not grow with it.
function blurF(f, w, h, sigma) {
  if (!(sigma > 0.4)) return Float32Array.from(f)
  const nb = 3
  const ideal = Math.sqrt((12 * sigma * sigma) / nb + 1)
  let wl = Math.floor(ideal)
  if (wl % 2 === 0) wl--
  const m = Math.round((12 * sigma * sigma - nb * wl * wl - 4 * nb * wl - 3 * nb) / (-4 * wl - 4))
  let a = Float32Array.from(f)
  const b = new Float32Array(f.length)
  for (let k = 0; k < nb; k++) {
    const r = ((k < m ? wl : wl + 2) - 1) / 2
    boxPass(a, b, w, h, r, true)
    boxPass(b, a, w, h, r, false)
  }
  return a
}

// Exact euclidean distance (Felzenszwalb) from every pixel to the nearest pixel where inside is set.
const FAR = 1e20
function edt1d(f, n, d, v, z) {
  let k = 0
  v[0] = 0
  z[0] = -Infinity
  z[1] = Infinity
  for (let q = 1; q < n; q++) {
    let s = (f[q] + q * q - (f[v[k]] + v[k] * v[k])) / (2 * q - 2 * v[k])
    while (s <= z[k]) {
      k--
      s = (f[q] + q * q - (f[v[k]] + v[k] * v[k])) / (2 * q - 2 * v[k])
    }
    k++
    v[k] = q
    z[k] = s
    z[k + 1] = Infinity
  }
  k = 0
  for (let q = 0; q < n; q++) {
    while (z[k + 1] < q) k++
    d[q] = (q - v[k]) * (q - v[k]) + f[v[k]]
  }
}

function edt(inside, w, h) {
  const m = Math.max(w, h)
  const f = new Float64Array(m)
  const d = new Float64Array(m)
  const v = new Int32Array(m)
  const z = new Float64Array(m + 1)
  const g = new Float64Array(w * h)
  for (let j = 0; j < g.length; j++) g[j] = inside[j] ? 0 : FAR
  for (let x = 0; x < w; x++) {
    for (let y = 0; y < h; y++) f[y] = g[y * w + x]
    edt1d(f, h, d, v, z)
    for (let y = 0; y < h; y++) g[y * w + x] = d[y]
  }
  const out = new Float32Array(w * h)
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) f[x] = g[y * w + x]
    edt1d(f, w, d, v, z)
    for (let x = 0; x < w; x++) out[y * w + x] = Math.sqrt(d[x])
  }
  return out
}

// A float field moved by (dx, dy); what slides in from outside is `fill`.
function shiftField(f, w, h, dx, dy, fill = 0) {
  const out = new Float32Array(w * h)
  for (let y = 0; y < h; y++) {
    const sy = y - dy
    for (let x = 0; x < w; x++) {
      const sx = x - dx
      out[y * w + x] = sx < 0 || sy < 0 || sx >= w || sy >= h ? fill : f[sy * w + sx]
    }
  }
  return out
}

// Points spaced no further apart than step.
function densify(pts, step) {
  const out = []
  for (let i = 0; i < pts.length; i++) {
    if (i === 0) {
      out.push(pts[0])
      continue
    }
    const [ax, ay] = pts[i - 1]
    const [bx, by] = pts[i]
    const k = Math.max(1, Math.ceil(Math.hypot(bx - ax, by - ay) / step))
    for (let t = 1; t <= k; t++) out.push([ax + ((bx - ax) * t) / k, ay + ((by - ay) * t) / k])
  }
  return out
}

// A stable 0..1 noise for pixel i (dissolve, grain), the same every time for the same seed.
export function hashNoise(i, seed = 1) {
  let h = (Math.imul(i | 0, 374761393) + Math.imul(seed | 0, 668265263)) | 0
  h = Math.imul(h ^ (h >>> 13), 1274126177)
  h ^= h >>> 16
  return (h >>> 0) / 4294967296
}

// ---------- 1. blend modes ----------

export const BLEND_MODES = [
  'normal',
  'dissolve',
  'darken',
  'multiply',
  'colorBurn',
  'linearBurn',
  'darkerColor',
  'lighten',
  'screen',
  'colorDodge',
  'linearDodge',
  'lighterColor',
  'overlay',
  'softLight',
  'hardLight',
  'vividLight',
  'linearLight',
  'pinLight',
  'hardMix',
  'difference',
  'exclusion',
  'subtract',
  'divide',
  'hue',
  'saturation',
  'color',
  'luminosity',
]

const burnCh = (s, d) => (d >= 1 ? 1 : s <= 0 ? 0 : 1 - Math.min(1, (1 - d) / s))
const dodgeCh = (s, d) => (d <= 0 ? 0 : s >= 1 ? 1 : Math.min(1, d / (1 - s)))
const hardLightCh = (s, d) => (s <= 0.5 ? 2 * s * d : 1 - 2 * (1 - s) * (1 - d))

// One separable channel, both 0..1: s is the layer, d the backdrop.
function blendChannel3(mode, s, d) {
  switch (mode) {
    case 'darken':
      return Math.min(s, d)
    case 'multiply':
      return s * d
    case 'colorBurn':
      return burnCh(s, d)
    case 'linearBurn':
      return Math.max(0, s + d - 1)
    case 'lighten':
      return Math.max(s, d)
    case 'screen':
      return s + d - s * d
    case 'colorDodge':
      return dodgeCh(s, d)
    case 'linearDodge':
      return Math.min(1, s + d)
    case 'overlay':
      return hardLightCh(d, s)
    case 'softLight': {
      if (s <= 0.5) return d - (1 - 2 * s) * d * (1 - d)
      const dd = d <= 0.25 ? ((16 * d - 12) * d + 4) * d : Math.sqrt(d)
      return d + (2 * s - 1) * (dd - d)
    }
    case 'hardLight':
      return hardLightCh(s, d)
    case 'vividLight':
      return s <= 0.5 ? burnCh(2 * s, d) : dodgeCh(2 * s - 1, d)
    case 'linearLight':
      return clamp01(d + 2 * s - 1)
    case 'pinLight':
      return s <= 0.5 ? Math.min(d, 2 * s) : Math.max(d, 2 * s - 1)
    case 'hardMix':
      return s + d >= 1 ? 1 : 0
    case 'difference':
      return Math.abs(s - d)
    case 'exclusion':
      return s + d - 2 * s * d
    case 'subtract':
      return Math.max(0, d - s)
    case 'divide':
      return s <= 0 ? (d <= 0 ? 0 : 1) : Math.min(1, d / s)
    default:
      return s
  }
}

const lum3 = (c) => 0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]

function clipColor(c) {
  const l = lum3(c)
  const n = Math.min(c[0], c[1], c[2])
  const x = Math.max(c[0], c[1], c[2])
  for (let k = 0; k < 3; k++) {
    if (n < 0) c[k] = l + ((c[k] - l) * l) / (l - n || 1e-9)
    if (x > 1) c[k] = l + ((c[k] - l) * (1 - l)) / (x - l || 1e-9)
  }
}

function setLum(c, l, out) {
  const d = l - lum3(c)
  out[0] = c[0] + d
  out[1] = c[1] + d
  out[2] = c[2] + d
  clipColor(out)
}

const sat3 = (c) => Math.max(c[0], c[1], c[2]) - Math.min(c[0], c[1], c[2])

function setSat(c, s, out) {
  const idx = [0, 1, 2].sort((a, b) => c[a] - c[b])
  const lo = c[idx[0]]
  const mid = c[idx[1]]
  const hi = c[idx[2]]
  if (hi > lo) {
    out[idx[1]] = ((mid - lo) * s) / (hi - lo)
    out[idx[2]] = s
  } else {
    out[idx[1]] = 0
    out[idx[2]] = 0
  }
  out[idx[0]] = 0
}

const TMP = [0, 0, 0]

// B(backdrop, source) for one colour, all 0..1, written into out.
export function blendRgb(mode, s, b, out = [0, 0, 0]) {
  switch (mode) {
    case 'hue':
      setSat(s, sat3(b), TMP)
      setLum(TMP, lum3(b), out)
      return out
    case 'saturation':
      setSat(b, sat3(s), TMP)
      setLum(TMP, lum3(b), out)
      return out
    case 'color':
      setLum(s, lum3(b), out)
      return out
    case 'luminosity':
      setLum(b, lum3(s), out)
      return out
    case 'darkerColor':
    case 'lighterColor': {
      const pickS = mode === 'darkerColor' ? lum3(s) < lum3(b) : lum3(s) > lum3(b)
      const c = pickS ? s : b
      out[0] = c[0]
      out[1] = c[1]
      out[2] = c[2]
      return out
    }
    default:
      for (let k = 0; k < 3; k++) out[k] = blendChannel3(mode, s[k], b[k])
      return out
  }
}

const CS = [0, 0, 0]
const CB = [0, 0, 0]
const BX = [0, 0, 0]

// src over dst, both [r, g, b, a] 0..255, with the W3C compositing formula and any of the 27 modes.
// noise (0..1) decides dissolve: the layer shows fully where noise is below its alpha.
export function blend3(mode, src, dst, opacity = 1, noise = 0.5, out = [0, 0, 0, 0]) {
  let as = (src[3] / 255) * opacity
  const ab = dst[3] / 255
  if (mode === 'dissolve') {
    if (as <= 0 || noise >= as) {
      out[0] = dst[0]
      out[1] = dst[1]
      out[2] = dst[2]
      out[3] = dst[3]
      return out
    }
    as = 1
  }
  const ao = as + ab * (1 - as)
  if (ao <= 0) {
    out[0] = out[1] = out[2] = out[3] = 0
    return out
  }
  for (let k = 0; k < 3; k++) {
    CS[k] = src[k] / 255
    CB[k] = dst[k] / 255
  }
  const mixedSrc = mode === 'normal' || mode === 'dissolve' || !mode ? CS : blendRgb(mode, CS, CB, BX)
  for (let k = 0; k < 3; k++) {
    const mixed = (1 - ab) * CS[k] + ab * mixedSrc[k]
    const co = as * mixed + ab * CB[k] * (1 - as)
    out[k] = Math.round(clamp((co / ao) * 255, 0, 255))
  }
  out[3] = Math.round(ao * 255)
  return out
}

const SRC = [0, 0, 0, 0]
const DST = [0, 0, 0, 0]
const RES = [0, 0, 0, 0]

// Same-size src onto dst in place, opacity times an optional per-pixel clip (0..1).
function blendOnto(dst, src, mode, opacity, clip, seed) {
  const o = dst.data
  const s = src.data
  for (let j = 0, i = 0; i < o.length; j++, i += 4) {
    if (!s[i + 3]) continue
    const op = clip ? opacity * clip[j] : opacity
    if (op <= 0) continue
    SRC[0] = s[i]
    SRC[1] = s[i + 1]
    SRC[2] = s[i + 2]
    SRC[3] = s[i + 3]
    DST[0] = o[i]
    DST[1] = o[i + 1]
    DST[2] = o[i + 2]
    DST[3] = o[i + 3]
    blend3(mode, SRC, DST, op, mode === 'dissolve' ? hashNoise(j, seed) : 0.5, RES)
    o[i] = RES[0]
    o[i + 1] = RES[1]
    o[i + 2] = RES[2]
    o[i + 3] = RES[3]
  }
  return dst
}

// ---------- 2. layer styles ----------

export const STYLE_DEFAULTS = {
  dropShadow: { color: '#000000', opacity: 0.75, blend: 'multiply', angle: 120, distance: 5, spread: 0, size: 5 },
  outerGlow: { color: '#ffffbe', opacity: 0.75, blend: 'screen', spread: 0, size: 5 },
  innerShadow: { color: '#000000', opacity: 0.75, blend: 'multiply', angle: 120, distance: 5, spread: 0, size: 5 },
  innerGlow: { color: '#ffffbe', opacity: 0.75, blend: 'screen', spread: 0, size: 5 },
  stroke: { color: '#000000', opacity: 1, blend: 'normal', width: 3, position: 'outside' },
  colorOverlay: { color: '#ff0000', opacity: 1, blend: 'normal' },
  gradientOverlay: {
    opacity: 1,
    blend: 'normal',
    angle: 90,
    stops: [
      { at: 0, color: '#000000' },
      { at: 1, color: '#ffffff' },
    ],
  },
  bevel: { opacity: 0.75, angle: 120, altitude: 30, size: 5, depth: 100, highlight: '#ffffff', shadow: '#000000', direction: 'up' },
}

// Bottom-to-top draw order of the effects under and over the layer.
export const STYLE_ORDER = { below: ['dropShadow', 'outerGlow'], above: ['gradientOverlay', 'colorOverlay', 'innerGlow', 'innerShadow', 'bevel', 'stroke'] }

// Spread grows the shape by size * spread% first; what is left of size softens it.
function spreadBlur(f, w, h, size, spread) {
  const sz = Math.max(0, num(size, 0))
  const sp = clamp(num(spread, 0), 0, 100) / 100
  let g = f
  if (sp > 0 && sz > 0) {
    const d = edt(Uint8Array.from(f, (v) => (v >= 0.5 ? 1 : 0)), w, h)
    const grow = sz * sp
    g = Float32Array.from(f, (v, j) => Math.max(v, clamp01(grow + 1 - d[j])))
  }
  return blurF(g, w, h, (sz * (1 - sp)) / 2)
}

function solidEffect(field, color, opacity, w, h) {
  const out = makePixels(w, h)
  const [r, g, b] = rgbOf(color)
  const op = clamp01(num(opacity, 1))
  const d = out.data
  for (let j = 0; j < field.length; j++) {
    const a = field[j] * op
    if (a <= 0) continue
    const i = j * 4
    d[i] = r
    d[i + 1] = g
    d[i + 2] = b
    d[i + 3] = a * 255
  }
  return out
}

// Layer effects from a layer's alpha. pixels may be smaller than width x height and placed at (x, y).
// A style draws unless its `on` is false. Returns the flattened effects under and over the layer,
// and the same as lists of { name, blend, pixels } so a compositor can blend each in its own mode.
export function renderStyles(pixels, styles = {}, opts = {}) {
  const w = Math.max(1, Math.round(opts.width ?? pixels.width))
  const h = Math.max(1, Math.round(opts.height ?? pixels.height))
  const ox = Math.round(opts.x || 0)
  const oy = Math.round(opts.y || 0)
  const n = w * h
  const A = new Float32Array(n)
  const s = pixels.data
  for (let y = 0; y < pixels.height; y++) {
    const Y = y + oy
    if (Y < 0 || Y >= h) continue
    for (let x = 0; x < pixels.width; x++) {
      const X = x + ox
      if (X < 0 || X >= w) continue
      A[Y * w + X] = s[(y * pixels.width + x) * 4 + 3] / 255
    }
  }
  const pick = (name) => {
    const v = styles?.[name]
    return v && v.on !== false ? { ...STYLE_DEFAULTS[name], ...v } : null
  }
  const offsetOf = (st) => {
    const rad = (num(st.angle, 120) * Math.PI) / 180
    const dist = num(st.distance, 0)
    return [Math.round(-Math.cos(rad) * dist), Math.round(Math.sin(rad) * dist)]
  }
  let dOut = null
  let dIn = null
  const distances = () => {
    if (dOut) return
    const inside = Uint8Array.from(A, (a) => (a >= 0.5 ? 1 : 0))
    dOut = edt(inside, w, h)
    dIn = edt(Uint8Array.from(inside, (v) => 1 - v), w, h)
  }
  const belowList = []
  const aboveList = []
  const push = (list, name, st, field, color = st.color, opacity = st.opacity, blend = st.blend) =>
    list.push({ name, blend: blend || 'normal', pixels: solidEffect(field, color, opacity, w, h) })

  let st = pick('dropShadow')
  if (st) {
    const [dx, dy] = offsetOf(st)
    push(belowList, 'dropShadow', st, spreadBlur(shiftField(A, w, h, dx, dy, 0), w, h, st.size, st.spread))
  }
  st = pick('outerGlow')
  if (st) push(belowList, 'outerGlow', st, spreadBlur(A, w, h, st.size, st.spread))

  st = pick('gradientOverlay')
  if (st) {
    let x0 = w
    let y0 = h
    let x1 = -1
    let y1 = -1
    for (let j = 0; j < n; j++) {
      if (!A[j]) continue
      const x = j % w
      const y = (j - x) / w
      if (x < x0) x0 = x
      if (x > x1) x1 = x
      if (y < y0) y0 = y
      if (y > y1) y1 = y
    }
    if (x1 >= 0) {
      const rad = (num(st.angle, 90) * Math.PI) / 180
      const ux = Math.cos(rad)
      const uy = -Math.sin(rad)
      const cx = (x0 + x1 + 1) / 2
      const cy = (y0 + y1 + 1) / 2
      const L = (Math.abs(ux) * (x1 + 1 - x0) + Math.abs(uy) * (y1 + 1 - y0)) / 2 || 1
      const radial = st.kind === 'radial'
      const from = radial ? [cx, cy] : [cx - ux * L, cy - uy * L]
      const to = [cx + ux * L, cy + uy * L]
      let stops = st.stops
      if (st.reverse) stops = (stops || []).map((p) => ({ ...p, at: 1 - num(p.at, 0) }))
      const grad = drawGradient(makePixels(w, h), { kind: radial ? 'radial' : 'linear', from, to, stops })
      const gd = grad.data
      const op = clamp01(num(st.opacity, 1))
      for (let j = 0; j < n; j++) gd[j * 4 + 3] = gd[j * 4 + 3] * A[j] * op
      aboveList.push({ name: 'gradientOverlay', blend: st.blend || 'normal', pixels: grad })
    }
  }
  st = pick('colorOverlay')
  if (st) push(aboveList, 'colorOverlay', st, A)
  st = pick('innerGlow')
  if (st) {
    const b = spreadBlur(Float32Array.from(A, (a) => 1 - a), w, h, st.size, st.spread)
    push(aboveList, 'innerGlow', st, Float32Array.from(b, (v, j) => A[j] * Math.min(1, 2 * v)))
  }
  st = pick('innerShadow')
  if (st) {
    const [dx, dy] = offsetOf(st)
    const inv = Float32Array.from(shiftField(A, w, h, dx, dy, 0), (v) => 1 - v)
    const b = spreadBlur(inv, w, h, st.size, st.spread)
    push(aboveList, 'innerShadow', st, Float32Array.from(b, (v, j) => v * A[j]))
  }
  st = pick('bevel')
  if (st) {
    const size = Math.max(1, num(st.size, 5))
    const H = blurF(A, w, h, size / 2)
    const az = (num(st.angle, 120) * Math.PI) / 180
    const alt = (num(st.altitude, 30) * Math.PI) / 180
    const Lx = Math.cos(az) * Math.cos(alt)
    const Ly = -Math.sin(az) * Math.cos(alt)
    const Lz = Math.sin(alt)
    const k = size * 1.25 * (num(st.depth, 100) / 100) * (st.direction === 'down' ? -1 : 1)
    const hi = new Float32Array(n)
    const lo = new Float32Array(n)
    for (let y = 0; y < h; y++) {
      const ya = y > 0 ? y - 1 : 0
      const yb = y < h - 1 ? y + 1 : h - 1
      for (let x = 0; x < w; x++) {
        const j = y * w + x
        if (!A[j]) continue
        const xa = x > 0 ? x - 1 : 0
        const xb = x < w - 1 ? x + 1 : w - 1
        const nx = -((H[y * w + xb] - H[y * w + xa]) / 2) * k
        const ny = -((H[yb * w + x] - H[ya * w + x]) / 2) * k
        const shade = (nx * Lx + ny * Ly + Lz) / Math.sqrt(nx * nx + ny * ny + 1) - Lz
        if (shade > 0) hi[j] = A[j] * clamp01(shade * 3)
        else lo[j] = A[j] * clamp01(-shade * 3)
      }
    }
    push(aboveList, 'bevelHighlight', st, hi, st.highlight, st.opacity, 'screen')
    push(aboveList, 'bevelShadow', st, lo, st.shadow, st.shadowOpacity ?? st.opacity, 'multiply')
  }
  st = pick('stroke')
  if (st) {
    distances()
    const W = Math.max(0, num(st.width ?? st.size, 3))
    const outer = (wd) => Float32Array.from(dOut, (d, j) => clamp01(wd + 1 - d) * (1 - A[j]))
    const inner = (wd) => Float32Array.from(dIn, (d, j) => (A[j] > 0 ? A[j] * clamp01(wd + 1 - d) : 0))
    let field
    if (st.position === 'inside') field = inner(W)
    else if (st.position === 'center') {
      const a = outer(W / 2)
      const b = inner(W / 2)
      field = Float32Array.from(a, (v, j) => Math.max(v, b[j]))
    } else field = outer(W)
    push(aboveList, 'stroke', st, field)
  }
  const flat = (list) => {
    const out = makePixels(w, h)
    for (const e of list) blendOnto(out, e.pixels, e.blend, 1, null, 1)
    return out
  }
  return { below: flat(belowList), above: flat(aboveList), belowList, aboveList }
}

// ---------- 3. compositing with groups, clipping and styles ----------

function placeLayer(p, x, y, W, H, mask) {
  const out = makePixels(W, H)
  const lx = Math.round(x || 0)
  const ly = Math.round(y || 0)
  const x0 = Math.max(0, lx)
  const x1 = Math.min(W, lx + p.width)
  if (x1 > x0) {
    for (let Y = Math.max(0, ly); Y < Math.min(H, ly + p.height); Y++) {
      const row = (Y - ly) * p.width
      out.data.set(p.data.subarray((row + x0 - lx) * 4, (row + x1 - lx) * 4), (Y * W + x0) * 4)
    }
  }
  return mask ? applyMask(out, mask) : out
}

function alphaField(p, opacity = 1) {
  const n = p.width * p.height
  const f = new Float32Array(n)
  for (let j = 0; j < n; j++) f[j] = (p.data[j * 4 + 3] / 255) * opacity
  return f
}

function hasStyles(styles) {
  return !!styles && Object.values(styles).some((v) => v && v.on !== false)
}

function paintStyled(canvas, content, styles, mode, opacity, clip, seed) {
  if (hasStyles(styles)) {
    const fx = renderStyles(content, styles, { width: canvas.width, height: canvas.height })
    for (const e of fx.belowList) blendOnto(canvas, e.pixels, e.blend, opacity, clip, seed)
    if (fx.aboveList.length) {
      // over-effects blend with the layer's own pixels, then the whole lot blends in the layer's mode
      content = clonePixels(content)
      for (const e of fx.aboveList) blendOnto(content, e.pixels, e.blend, 1, null, seed)
    }
  }
  blendOnto(canvas, content, mode, opacity, clip, seed)
}

// One adjustment ({ type, params }) by name: every engine3 adjustment and filter, then engine2's
// curves / levels / hsl / adjust.
export function applyAdjustment3(pixels, adjustment, mask = null) {
  const type = adjustment?.type
  const params = adjustment?.params || {}
  const fn = ADJUST_FNS[type] || FILTER_FNS[type]
  if (fn) return fn(pixels, params, mask)
  return mixByMask(pixels, applyAdjustment(pixels, adjustment), mask)
}

// Layers bottom-to-top onto a transparent width x height result. Layer shape:
// { id, kind: 'image'|'text'|'adjustment'|'group', parent?, pixels?, mask?, opacity, blend, visible,
//   clipped?, styles?, adjustment?, x?, y? }. A group's children carry parent = its id; a group is
// isolated (children composite alone, then blend in with the group's opacity, blend and mask) unless
// its blend is 'passThrough'. A clipped layer shows only where the nearest unclipped layer below it
// (same parent) has alpha, times that layer's opacity. Masks are document-sized.
export function compose3(layers, width, height) {
  const W = Math.max(1, Math.round(width))
  const H = Math.max(1, Math.round(height))
  const n = W * H
  const all = (layers || []).filter(Boolean)
  const groupIds = new Set(all.filter((l) => l.kind === 'group' && l.id != null).map((l) => l.id))
  const kids = new Map()
  for (const l of all) {
    const p = l.parent != null && groupIds.has(l.parent) && l.parent !== l.id ? l.parent : null
    if (!kids.has(p)) kids.set(p, [])
    kids.get(p).push(l)
  }

  const drawAdjustment = (layer, canvas, opacity, clip) => {
    const mask = layer.mask || null
    if (opacity > 0) {
      const a = applyAdjustment3(canvas, layer.adjustment).data
      const o = canvas.data
      for (let j = 0, i = 0; j < n; j++, i += 4) {
        const t = opacity * (mask ? mask[j] / 255 : 1) * (clip ? clip[j] : 1)
        if (!t) continue
        o[i] = o[i] + (a[i] - o[i]) * t
        o[i + 1] = o[i + 1] + (a[i + 1] - o[i + 1]) * t
        o[i + 2] = o[i + 2] + (a[i + 2] - o[i + 2]) * t
      }
    }
    const f = new Float32Array(n)
    for (let j = 0; j < n; j++) f[j] = opacity * (mask ? mask[j] / 255 : 1)
    return f
  }

  const drawGroup = (layer, canvas, opacity, clip, seed, needBase) => {
    const mask = layer.mask || null
    if (layer.blend === 'passThrough' && !hasStyles(layer.styles)) {
      const before = clonePixels(canvas)
      drawList(layer.id, canvas)
      const b = before.data
      const o = canvas.data
      for (let j = 0, i = 0; j < n; j++, i += 4) {
        const t = opacity * (mask ? mask[j] / 255 : 1) * (clip ? clip[j] : 1)
        if (t >= 1) continue
        for (let k = 0; k < 4; k++) o[i + k] = b[i + k] + (o[i + k] - b[i + k]) * t
      }
      if (!needBase) return null
      const iso = makePixels(W, H)
      drawList(layer.id, iso)
      return alphaField(mask ? applyMask(iso, mask) : iso, opacity)
    }
    const iso = makePixels(W, H)
    drawList(layer.id, iso)
    const content = mask ? applyMask(iso, mask) : iso
    const mode = !layer.blend || layer.blend === 'passThrough' ? 'normal' : layer.blend
    if (opacity > 0) paintStyled(canvas, content, layer.styles, mode, opacity, clip, seed)
    return alphaField(content, opacity)
  }

  const drawList = (parentId, canvas) => {
    const list = kids.get(parentId) || []
    let base = null
    list.forEach((layer, k) => {
      const clip = layer.clipped && base ? base : null
      if (layer.visible === false) {
        if (!layer.clipped) base = new Float32Array(n)
        return
      }
      const opacity = clamp(num(layer.opacity, 1), 0, 1)
      const seed = all.indexOf(layer) + 1
      let alpha = null
      if (layer.kind === 'group') alpha = drawGroup(layer, canvas, opacity, clip, seed, !!list[k + 1]?.clipped)
      else if (layer.kind === 'adjustment') alpha = drawAdjustment(layer, canvas, opacity, clip)
      else if (layer.pixels) {
        const placed = placeLayer(layer.pixels, layer.x, layer.y, W, H, layer.mask || null)
        if (opacity > 0) paintStyled(canvas, placed, layer.styles, layer.blend || 'normal', opacity, clip, seed)
        alpha = alphaField(placed, opacity)
      }
      if (!layer.clipped) base = alpha || new Float32Array(n)
    })
  }

  const out = makePixels(W, H)
  drawList(null, out)
  return out
}

// ---------- 4. adjustments ----------

// vibrance -100..100 boosts dull colours more than strong ones; saturation -100..100 moves all.
export function vibrance(pixels, { vibrance: vib = 0, saturation = 0 } = {}, mask = null) {
  const v = num(vib, 0) / 100
  const s = num(saturation, 0) / 100
  if (!v && !s) return clonePixels(pixels)
  return mapRgb(pixels, mask, (r, g, b, o) => {
    const chroma = (Math.max(r, g, b) - Math.min(r, g, b)) / 255
    const k = Math.max(0, (1 + s) * (1 + v * (1 - chroma)))
    const L = lumaOf(r, g, b)
    o[0] = clamp255(L + (r - L) * k)
    o[1] = clamp255(L + (g - L) * k)
    o[2] = clamp255(L + (b - L) * k)
  })
}

// Each tone range is [cyan-red, magenta-green, yellow-blue], -100..100.
export function colorBalance(pixels, { shadows = [0, 0, 0], midtones = [0, 0, 0], highlights = [0, 0, 0], preserveLuminosity = true } = {}, mask = null) {
  const S = [0, 1, 2].map((k) => num(shadows?.[k], 0))
  const M = [0, 1, 2].map((k) => num(midtones?.[k], 0))
  const Hh = [0, 1, 2].map((k) => num(highlights?.[k], 0))
  return mapRgb(pixels, mask, (r, g, b, o) => {
    const L0 = lumaOf(r, g, b)
    const l = L0 / 255
    const ws = 1 - smooth(0, 0.5, l)
    const wh = smooth(0.5, 1, l)
    const wm = 1 - ws - wh
    const c = [r, g, b]
    for (let k = 0; k < 3; k++) c[k] += ((ws * S[k] + wm * M[k] + wh * Hh[k]) / 100) * 80
    if (preserveLuminosity) {
      const d = L0 - lumaOf(c[0], c[1], c[2])
      c[0] += d
      c[1] += d
      c[2] += d
    }
    o[0] = clamp255(c[0])
    o[1] = clamp255(c[1])
    o[2] = clamp255(c[2])
  })
}

// Photoshop's Black & White: grey = min + (mid - min) * secondary weight + (max - mid) * primary weight.
// Weights are percents; tint (a hex colour) colours the grey keeping its lightness.
export function blackWhite(pixels, params = {}, mask = null) {
  const wt = {
    reds: num(params.reds, 40) / 100,
    yellows: num(params.yellows, 60) / 100,
    greens: num(params.greens, 40) / 100,
    cyans: num(params.cyans, 60) / 100,
    blues: num(params.blues, 20) / 100,
    magentas: num(params.magentas, 80) / 100,
  }
  const primary = [wt.reds, wt.greens, wt.blues]
  const tint = params.tint ? rgbOf(params.tint).slice(0, 3).map((v) => v / 255) : null
  const out3 = [0, 0, 0]
  return mapRgb(pixels, mask, (r, g, b, o) => {
    const c = [r, g, b]
    const idx = [0, 1, 2].sort((a, z) => c[a] - c[z])
    const lo = c[idx[0]]
    const mid = c[idx[1]]
    const hi = c[idx[2]]
    const pair = idx[1] + idx[2]
    const secondary = pair === 1 ? wt.yellows : pair === 3 ? wt.cyans : wt.magentas
    const grey = clamp255(lo + (mid - lo) * secondary + (hi - mid) * primary[idx[2]])
    if (tint) {
      setLum(tint, grey / 255, out3)
      o[0] = out3[0] * 255
      o[1] = out3[1] * 255
      o[2] = out3[2] * 255
    } else o[0] = o[1] = o[2] = grey
  })
}

// A coloured filter over the lens: density 0..100; preserving luminosity keeps the brightness.
export function photoFilter(pixels, { color = '#ec8a00', density = 25, preserveLuminosity = true } = {}, mask = null) {
  const [fr, fg, fb] = rgbOf(color)
  const t = clamp(num(density, 25), 0, 100) / 100
  return mapRgb(pixels, mask, (r, g, b, o) => {
    let R = r + ((r * fr) / 255 - r) * t + (fr - r) * t * 0.3
    let G = g + ((g * fg) / 255 - g) * t + (fg - g) * t * 0.3
    let B = b + ((b * fb) / 255 - b) * t + (fb - b) * t * 0.3
    if (preserveLuminosity) {
      const d = lumaOf(r, g, b) - lumaOf(R, G, B)
      R += d
      G += d
      B += d
    }
    o[0] = clamp255(R)
    o[1] = clamp255(G)
    o[2] = clamp255(B)
  })
}

// Brightness through a gradient: stops [{ at 0..1, color }], dark end first (reverse flips it).
export function gradientMap(pixels, { stops = null, reverse = false } = {}, mask = null) {
  let list = stops?.length ? stops : [{ at: 0, color: '#000000' }, { at: 1, color: '#ffffff' }]
  if (reverse) list = list.map((p) => ({ ...p, at: 1 - num(p.at, 0) }))
  const strip = drawGradient(makePixels(256, 1), { from: [0.5, 0], to: [255.5, 0], stops: list }).data
  return mapRgb(pixels, mask, (r, g, b, o) => {
    const i = Math.round(lumaOf(r, g, b)) * 4
    o[0] = strip[i]
    o[1] = strip[i + 1]
    o[2] = strip[i + 2]
  })
}

export const SELECTIVE_RANGES = ['reds', 'yellows', 'greens', 'cyans', 'blues', 'magentas', 'whites', 'neutrals', 'blacks']

// Selective colour: each range gets { c, m, y, k } shifts, -100..100. Relative scales the ink that
// is already there; absolute adds a share of the full range.
export function selectiveColor(pixels, params = {}, mask = null) {
  const relative = params.relative !== false
  const ranges = SELECTIVE_RANGES.map((name) => {
    const p = params[name] || {}
    const k = num(p.k, 0)
    return { name, adj: [num(p.c, 0) + k, num(p.m, 0) + k, num(p.y, 0) + k].map((v) => v / 100) }
  }).filter((r) => r.adj.some(Boolean))
  if (!ranges.length) return clonePixels(pixels)
  return mapRgb(pixels, mask, (r, g, b, o) => {
    const c = [r, g, b]
    const mx = Math.max(r, g, b)
    const mn = Math.min(r, g, b)
    const md = r + g + b - mx - mn
    const delta = [0, 0, 0]
    for (const { name, adj } of ranges) {
      let amount = 0
      if (name === 'reds') amount = r === mx && mx > mn ? mx - md : 0
      else if (name === 'greens') amount = g === mx && r !== mx && mx > mn ? mx - md : 0
      else if (name === 'blues') amount = b === mx && r !== mx && g !== mx && mx > mn ? mx - md : 0
      else if (name === 'yellows') amount = b === mn && mx > mn ? md - mn : 0
      else if (name === 'cyans') amount = r === mn && b !== mn && mx > mn ? md - mn : 0
      else if (name === 'magentas') amount = g === mn && b !== mn && r !== mn && mx > mn ? md - mn : 0
      else if (name === 'whites') amount = mn > 128 ? (mn - 128) * 2 : 0
      else if (name === 'blacks') amount = mx < 128 ? (128 - mx) * 2 : 0
      else amount = Math.max(0, 255 - Math.abs(mx - 128) - Math.abs(mn - 128))
      if (!amount) continue
      const w = Math.min(255, amount) / 255
      for (let ch = 0; ch < 3; ch++) {
        const ink = 255 - c[ch]
        delta[ch] -= (relative ? ink : 255) * adj[ch] * w
      }
    }
    o[0] = clamp255(r + delta[0])
    o[1] = clamp255(g + delta[1])
    o[2] = clamp255(b + delta[2])
  })
}

// Each output channel is [red%, green%, blue%, constant%] of the input; monochrome uses `red` for all.
export function channelMixer(pixels, { red = [100, 0, 0, 0], green = [0, 100, 0, 0], blue = [0, 0, 100, 0], monochrome = false } = {}, mask = null) {
  const rows = (monochrome ? [red, red, red] : [red, green, blue]).map((row) => [0, 1, 2, 3].map((k) => num(row?.[k], 0) / 100))
  return mapRgb(pixels, mask, (r, g, b, o) => {
    for (let k = 0; k < 3; k++) {
      const m = rows[k]
      o[k] = clamp255(r * m[0] + g * m[1] + b * m[2] + 255 * m[3])
    }
  })
}

export function posterize(pixels, { levels = 4 } = {}, mask = null) {
  const n = clamp(Math.round(num(levels, 4)), 2, 255)
  const lut = Uint8Array.from({ length: 256 }, (_, v) => Math.round((Math.round((v / 255) * (n - 1)) / (n - 1)) * 255))
  return lut3(pixels, lut, lut, lut, mask)
}

export function threshold(pixels, { level = 128 } = {}, mask = null) {
  const t = num(level, 128)
  return mapRgb(pixels, mask, (r, g, b, o) => {
    o[0] = o[1] = o[2] = Math.round(lumaOf(r, g, b)) >= t ? 255 : 0
  })
}

export function invert(pixels, params = {}, mask = null) {
  const lut = Uint8Array.from({ length: 256 }, (_, v) => 255 - v)
  return lut3(pixels, lut, lut, lut, mask)
}

const toLinear = (v) => (v <= 0.04045 ? v / 12.92 : Math.pow((v + 0.055) / 1.055, 2.4))
const toSrgb = (v) => (v <= 0.0031308 ? v * 12.92 : 1.055 * Math.pow(v, 1 / 2.4) - 0.055)

// Exposure in stops, offset -0.5..0.5 and gamma 0.01..9.99, worked in linear light like Photoshop.
export function exposure(pixels, { exposure: ev = 0, offset = 0, gamma = 1 } = {}, mask = null) {
  const gain = Math.pow(2, num(ev, 0))
  const off = num(offset, 0)
  const g = clamp(num(gamma, 1), 0.01, 9.99)
  const lut = Uint8Array.from({ length: 256 }, (_, v) => {
    const lin = Math.max(0, toLinear(v / 255) * gain + off)
    return Math.round(clamp01(toSrgb(clamp01(Math.pow(lin, 1 / g)))) * 255)
  })
  return lut3(pixels, lut, lut, lut, mask)
}

// Local tone: shadows 0..100 lifts what sits in dark surroundings, highlights 0..100 tames what sits
// in bright ones; radius (px) is how wide "surroundings" is.
export function shadowsHighlights(pixels, { shadows = 35, highlights = 0, radius = 30 } = {}, mask = null) {
  const sh = clamp(num(shadows, 35), 0, 100) / 100
  const hl = clamp(num(highlights, 0), 0, 100) / 100
  if (!sh && !hl) return clonePixels(pixels)
  const { width: w, height: h, data: s } = pixels
  const L = new Float32Array(w * h)
  for (let j = 0, i = 0; j < L.length; j++, i += 4) L[j] = lumaOf(s[i], s[i + 1], s[i + 2]) / 255
  const Lb = blurF(L, w, h, Math.max(1, num(radius, 30)) / 2)
  return mapRgb(pixels, mask, (r, g, b, o, j) => {
    const lift = sh * (1 - smooth(0, 0.5, Lb[j]))
    const tame = hl * smooth(0.5, 1, Lb[j])
    const c = [r, g, b]
    for (let k = 0; k < 3; k++) {
      let v = c[k] / 255
      if (lift) v = Math.pow(v, 1 / (1 + 2 * lift))
      if (tame) v -= tame * 0.5 * v * v
      o[k] = clamp255(v * 255)
    }
  })
}

// brightness -150..150 (a midtone curve), contrast -50..100 (around middle grey).
export function brightnessContrast(pixels, { brightness = 0, contrast = 0 } = {}, mask = null) {
  const b = clamp(num(brightness, 0), -150, 150) / 150
  const c = clamp(num(contrast, 0), -50, 100) / 100
  const gamma = Math.pow(2, -b)
  const lut = Uint8Array.from({ length: 256 }, (_, v) => {
    const v1 = 255 * Math.pow(v / 255, gamma)
    return Math.round(clamp255(128 + (v1 - 128) * (1 + c * (c > 0 ? 1.5 : 1))))
  })
  return lut3(pixels, lut, lut, lut, mask)
}

// Six film looks, each a small function (r, g, b in 0..1) -> [r, g, b].
const sCurve = (x, k) => clamp01(x - (k * Math.sin(2 * Math.PI * x)) / (2 * Math.PI))
const satMix = (r, g, b, s) => {
  const l = lumaOf(r, g, b)
  return [l + (r - l) * s, l + (g - l) * s, l + (b - l) * s]
}
export const LOOKS = {
  portra: (r, g, b) => {
    const [R, G, B] = satMix(r, g, b, 0.88)
    return [0.05 + 0.93 * sCurve(clamp01(R * 1.03), 0.25), 0.045 + 0.93 * sCurve(clamp01(G), 0.25), 0.06 + 0.88 * sCurve(clamp01(B * 0.96), 0.25)].map(clamp01)
  },
  velvia: (r, g, b) => {
    const [R, G, B] = satMix(r, g, b, 1.45)
    return [sCurve(clamp01(R), 0.6), sCurve(clamp01(G * 1.02), 0.6), clamp01(sCurve(clamp01(B), 0.6) + 0.03 * (1 - lumaOf(r, g, b)))]
  },
  kodachrome: (r, g, b) => {
    const [R, G, B] = satMix(r, g, b, 1.2)
    return [clamp01(sCurve(clamp01(R * 1.06), 0.5) - 0.02), clamp01(sCurve(clamp01(G), 0.5) - 0.02), clamp01(sCurve(clamp01(B * 0.94), 0.5) - 0.02)]
  },
  tealOrange: (r, g, b) => {
    const d = (lumaOf(r, g, b) - 0.5) * 2
    const [R, G, B] = satMix(r, g, b, 1.15)
    return [sCurve(clamp01(R + 0.12 * d), 0.3), sCurve(clamp01(G + 0.02 * d + 0.02 * (1 - Math.abs(d))), 0.3), sCurve(clamp01(B - 0.12 * d), 0.3)]
  },
  bleachBypass: (r, g, b) => satMix(r, g, b, 0.45).map((v) => sCurve(clamp01(v), 0.7)),
  noir: (r, g, b) => {
    const l = sCurve(clamp01((lumaOf(r, g, b) - 0.05) / 0.95), 0.9)
    return [l, l, l]
  },
}

const LUT_N = 17
const lutCache = new Map()
function lookTable(name) {
  if (lutCache.has(name)) return lutCache.get(name)
  const fn = LOOKS[name]
  const t = new Float32Array(LUT_N * LUT_N * LUT_N * 3)
  let q = 0
  for (let b = 0; b < LUT_N; b++) {
    for (let g = 0; g < LUT_N; g++) {
      for (let r = 0; r < LUT_N; r++) {
        const o = fn(r / (LUT_N - 1), g / (LUT_N - 1), b / (LUT_N - 1))
        t[q++] = o[0]
        t[q++] = o[1]
        t[q++] = o[2]
      }
    }
  }
  lutCache.set(name, t)
  return t
}

// A film look through a 17-cube LUT (trilinear); amount 0..1 mixes it in.
export function colorLookup(pixels, { look = 'portra', amount = 1 } = {}, mask = null) {
  if (!LOOKS[look]) return clonePixels(pixels)
  const t = lookTable(look)
  const amt = clamp01(num(amount, 1))
  const N = LUT_N - 1
  const at = (ri, gi, bi) => ((bi * LUT_N + gi) * LUT_N + ri) * 3
  return mapRgb(pixels, mask, (r, g, b, o) => {
    const fr = (r / 255) * N
    const fg = (g / 255) * N
    const fb = (b / 255) * N
    const r0 = Math.min(Math.floor(fr), N - 1)
    const g0 = Math.min(Math.floor(fg), N - 1)
    const b0 = Math.min(Math.floor(fb), N - 1)
    const tr = fr - r0
    const tg = fg - g0
    const tb = fb - b0
    const src = [r, g, b]
    for (let c = 0; c < 3; c++) {
      const c00 = t[at(r0, g0, b0) + c] * (1 - tr) + t[at(r0 + 1, g0, b0) + c] * tr
      const c10 = t[at(r0, g0 + 1, b0) + c] * (1 - tr) + t[at(r0 + 1, g0 + 1, b0) + c] * tr
      const c01 = t[at(r0, g0, b0 + 1) + c] * (1 - tr) + t[at(r0 + 1, g0, b0 + 1) + c] * tr
      const c11 = t[at(r0, g0 + 1, b0 + 1) + c] * (1 - tr) + t[at(r0 + 1, g0 + 1, b0 + 1) + c] * tr
      const v = ((c00 * (1 - tg) + c10 * tg) * (1 - tb) + (c01 * (1 - tg) + c11 * tg) * tb) * 255
      o[c] = clamp255(src[c] + (v - src[c]) * amt)
    }
  })
}

const ADJUST_FNS = { vibrance, colorBalance, blackWhite, photoFilter, gradientMap, selectiveColor, channelMixer, posterize, threshold, invert, exposure, shadowsHighlights, brightnessContrast, colorLookup }

// Every engine3 adjustment: its label and default params (the panel builds from these).
export const ADJUSTMENTS = {
  vibrance: { label: 'Vibrance', params: { vibrance: 0, saturation: 0 } },
  colorBalance: { label: 'Color Balance', params: { shadows: [0, 0, 0], midtones: [0, 0, 0], highlights: [0, 0, 0], preserveLuminosity: true } },
  blackWhite: { label: 'Black & White', params: { reds: 40, yellows: 60, greens: 40, cyans: 60, blues: 20, magentas: 80, tint: null } },
  photoFilter: { label: 'Photo Filter', params: { color: '#ec8a00', density: 25, preserveLuminosity: true } },
  gradientMap: {
    label: 'Gradient Map',
    params: {
      stops: [
        { at: 0, color: '#000000' },
        { at: 1, color: '#ffffff' },
      ],
      reverse: false,
    },
  },
  selectiveColor: { label: 'Selective Color', params: { relative: true, ...Object.fromEntries(SELECTIVE_RANGES.map((r) => [r, { c: 0, m: 0, y: 0, k: 0 }])) } },
  channelMixer: { label: 'Channel Mixer', params: { red: [100, 0, 0, 0], green: [0, 100, 0, 0], blue: [0, 0, 100, 0], monochrome: false } },
  posterize: { label: 'Posterize', params: { levels: 4 } },
  threshold: { label: 'Threshold', params: { level: 128 } },
  invert: { label: 'Invert', params: {} },
  exposure: { label: 'Exposure', params: { exposure: 0, offset: 0, gamma: 1 } },
  shadowsHighlights: { label: 'Shadows/Highlights', params: { shadows: 35, highlights: 0, radius: 30 } },
  brightnessContrast: { label: 'Brightness/Contrast', params: { brightness: 0, contrast: 0 } },
  colorLookup: { label: 'Color Lookup', params: { look: 'portra', amount: 1 } },
}

// ---------- 5. filters ----------

// Streaks along angle (degrees, 0 = to the right) over distance px.
export function motionBlur(pixels, { angle = 0, distance = 10 } = {}, mask = null) {
  const dist = Math.max(0, num(distance, 10))
  if (dist < 1) return clonePixels(pixels)
  const { width: w, height: h } = pixels
  const f = premul(pixels)
  const out = makePixels(w, h)
  const rad = (num(angle, 0) * Math.PI) / 180
  const ux = Math.cos(rad)
  const uy = -Math.sin(rad)
  const steps = Math.max(2, Math.round(dist))
  const offs = []
  for (let k = 0; k <= steps; k++) {
    const t = -dist / 2 + (dist * k) / steps
    offs.push([Math.round(ux * t), Math.round(uy * t)])
  }
  const acc = [0, 0, 0, 0]
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      acc[0] = acc[1] = acc[2] = acc[3] = 0
      for (const [ox, oy] of offs) {
        const i = (clamp(y + oy, 0, h - 1) * w + clamp(x + ox, 0, w - 1)) * 4
        acc[0] += f[i]
        acc[1] += f[i + 1]
        acc[2] += f[i + 2]
        acc[3] += f[i + 3]
      }
      writeAcc(out.data, (y * w + x) * 4, acc, offs.length)
    }
  }
  return mixByMask(pixels, out, mask)
}

// Spin (around the centre, amount = degrees of sweep) or zoom (towards it, amount = % of distance).
export function radialBlur(pixels, { mode = 'spin', center = null, amount = 10 } = {}, mask = null) {
  const amt = clamp(num(amount, 10), 0, 100)
  if (!amt) return clonePixels(pixels)
  const { width: w, height: h } = pixels
  const [cx, cy] = center ? toPt(center) : [w / 2, h / 2]
  const f = premul(pixels)
  const out = makePixels(w, h)
  const n = clamp(Math.ceil(amt * 0.6), 4, 40)
  const acc = [0, 0, 0, 0]
  const span = mode === 'zoom' ? (amt / 100) * 0.5 : (amt * Math.PI) / 180
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      acc[0] = acc[1] = acc[2] = acc[3] = 0
      const px = x + 0.5 - cx
      const py = y + 0.5 - cy
      for (let k = 0; k < n; k++) {
        const t = k / (n - 1) - 0.5
        let sx
        let sy
        if (mode === 'zoom') {
          const sc = 1 - span * (t + 0.5)
          sx = cx + px * sc
          sy = cy + py * sc
        } else {
          const a = span * t
          const ca = Math.cos(a)
          const sa = Math.sin(a)
          sx = cx + px * ca - py * sa
          sy = cy + px * sa + py * ca
        }
        samplePremul(f, w, h, sx - 0.5, sy - 0.5, acc)
      }
      writeAcc(out.data, (y * w + x) * 4, acc, n)
    }
  }
  return mixByMask(pixels, out, mask)
}

// Smooths flat areas but keeps edges: neighbours differing by more than threshold barely count.
export function surfaceBlur(pixels, { radius = 5, threshold: th = 15 } = {}, mask = null) {
  const r = clamp(Math.round(num(radius, 5)), 0, 50)
  if (!r) return clonePixels(pixels)
  const t = Math.max(1, num(th, 15)) * 2.5
  const { width: w, height: h, data: s } = pixels
  const out = clonePixels(pixels)
  const d = out.data
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * 4
      for (let c = 0; c < 3; c++) {
        const v0 = s[i + c]
        let sum = 0
        let wsum = 0
        for (let yy = Math.max(0, y - r); yy <= Math.min(h - 1, y + r); yy++) {
          for (let xx = Math.max(0, x - r); xx <= Math.min(w - 1, x + r); xx++) {
            const v = s[(yy * w + xx) * 4 + c]
            const wt = 1 - Math.abs(v - v0) / t
            if (wt <= 0) continue
            sum += v * wt
            wsum += wt
          }
        }
        d[i + c] = sum / wsum
      }
    }
  }
  return mixByMask(pixels, out, mask)
}

// Depth-of-field blur: each pixel's blur grows with |depth - focus| (depthMask 0..255, focus 0..255).
// Without a depth mask the whole picture gets the full radius.
export function lensBlur(pixels, { depthMask = null, radius = 10, focus = 0 } = {}, mask = null) {
  const R = Math.max(0, num(radius, 10))
  if (!R) return clonePixels(pixels)
  const levels = [pixels]
  for (let k = 1; k <= 4; k++) levels.push(gaussianBlur(pixels, ((R * k) / 4) * 0.6))
  if (!depthMask) return mixByMask(pixels, levels[4], mask)
  const out = clonePixels(pixels)
  const d = out.data
  const fc = num(focus, 0)
  for (let j = 0, i = 0; j < depthMask.length; j++, i += 4) {
    const lv = (Math.abs(depthMask[j] - fc) / 255) * 4
    const a = Math.min(3, Math.floor(lv))
    const t = lv - a
    const A = levels[a].data
    const B = levels[a + 1].data
    for (let c = 0; c < 4; c++) d[i + c] = A[i + c] + (B[i + c] - A[i + c]) * t
  }
  return mixByMask(pixels, out, mask)
}

// Keeps only detail finer than radius, around middle grey.
export function highPass(pixels, { radius = 10 } = {}, mask = null) {
  const b = gaussianBlur(pixels, Math.max(0.5, num(radius, 10))).data
  return mapRgb(pixels, mask, (r, g, bl, o, j) => {
    const i = j * 4
    o[0] = clamp255(128 + r - b[i])
    o[1] = clamp255(128 + g - b[i + 1])
    o[2] = clamp255(128 + bl - b[i + 2])
  })
}

// Mosaic: each cell x cell square becomes its average colour.
export function pixelate(pixels, { cell = 8 } = {}, mask = null) {
  const c = Math.max(1, Math.round(num(cell, 8)))
  const { width: w, height: h } = pixels
  const f = premul(pixels)
  const out = makePixels(w, h)
  const acc = [0, 0, 0, 0]
  for (let y0 = 0; y0 < h; y0 += c) {
    for (let x0 = 0; x0 < w; x0 += c) {
      acc[0] = acc[1] = acc[2] = acc[3] = 0
      let n = 0
      for (let y = y0; y < Math.min(h, y0 + c); y++) {
        for (let x = x0; x < Math.min(w, x0 + c); x++) {
          const i = (y * w + x) * 4
          for (let k = 0; k < 4; k++) acc[k] += f[i + k]
          n++
        }
      }
      for (let y = y0; y < Math.min(h, y0 + c); y++) for (let x = x0; x < Math.min(w, x0 + c); x++) writeAcc(out.data, (y * w + x) * 4, acc, n)
    }
  }
  return mixByMask(pixels, out, mask)
}

// Grey relief lit from angle; height in px, amount in percent.
export function emboss(pixels, { angle = 135, height = 3, amount = 100 } = {}, mask = null) {
  const { width: w, height: h, data: s } = pixels
  const rad = (num(angle, 135) * Math.PI) / 180
  const hh = Math.max(1, num(height, 3))
  const dx = Math.round(Math.cos(rad) * hh)
  const dy = Math.round(-Math.sin(rad) * hh)
  const k = num(amount, 100) / 100
  const L = (x, y) => {
    const i = (clamp(y, 0, h - 1) * w + clamp(x, 0, w - 1)) * 4
    return lumaOf(s[i], s[i + 1], s[i + 2])
  }
  return mapRgb(pixels, mask, (r, g, b, o, j) => {
    const x = j % w
    const y = (j - x) / w
    o[0] = o[1] = o[2] = clamp255(128 + (L(x + dx, y + dy) - L(x - dx, y - dy)) * k)
  })
}

// Dark lines where colour changes, on white (Sobel per channel).
export function findEdges(pixels, params = {}, mask = null) {
  const { width: w, height: h, data: s } = pixels
  const v = (x, y, c) => s[(clamp(y, 0, h - 1) * w + clamp(x, 0, w - 1)) * 4 + c]
  return mapRgb(pixels, mask, (r, g, b, o, j) => {
    const x = j % w
    const y = (j - x) / w
    for (let c = 0; c < 3; c++) {
      const gx = v(x + 1, y - 1, c) + 2 * v(x + 1, y, c) + v(x + 1, y + 1, c) - v(x - 1, y - 1, c) - 2 * v(x - 1, y, c) - v(x - 1, y + 1, c)
      const gy = v(x - 1, y + 1, c) + 2 * v(x, y + 1, c) + v(x + 1, y + 1, c) - v(x - 1, y - 1, c) - 2 * v(x, y - 1, c) - v(x + 1, y - 1, c)
      o[c] = clamp255(255 - Math.hypot(gx, gy) / 2)
    }
  })
}

// Oil paint: each pixel takes the average colour of the most common brightness band around it.
export function oilPaint(pixels, { radius = 3, levels = 20 } = {}, mask = null) {
  const r = clamp(Math.round(num(radius, 3)), 1, 20)
  const nl = clamp(Math.round(num(levels, 20)), 2, 255)
  const { width: w, height: h, data: s } = pixels
  const band = new Uint8Array(w * h)
  for (let j = 0, i = 0; j < band.length; j++, i += 4) band[j] = Math.min(nl - 1, Math.floor((lumaOf(s[i], s[i + 1], s[i + 2]) / 256) * nl))
  const cnt = new Int32Array(nl)
  const sr = new Float64Array(nl)
  const sg = new Float64Array(nl)
  const sb = new Float64Array(nl)
  return mapRgb(pixels, mask, (rr, gg, bb, o, j) => {
    const x = j % w
    const y = (j - x) / w
    cnt.fill(0)
    sr.fill(0)
    sg.fill(0)
    sb.fill(0)
    for (let yy = Math.max(0, y - r); yy <= Math.min(h - 1, y + r); yy++) {
      for (let xx = Math.max(0, x - r); xx <= Math.min(w - 1, x + r); xx++) {
        const q = yy * w + xx
        const k = band[q]
        cnt[k]++
        sr[k] += s[q * 4]
        sg[k] += s[q * 4 + 1]
        sb[k] += s[q * 4 + 2]
      }
    }
    let best = 0
    for (let k = 1; k < nl; k++) if (cnt[k] > cnt[best]) best = k
    o[0] = sr[best] / cnt[best]
    o[1] = sg[best] / cnt[best]
    o[2] = sb[best] / cnt[best]
  })
}

// Film grain: mono noise (amount 0..100), size >= 1 makes the grain coarser; strongest in midtones.
export function addGrain(pixels, { amount = 20, size = 1, seed = 1 } = {}, mask = null) {
  const amt = clamp(num(amount, 20), 0, 100) / 100
  if (!amt) return clonePixels(pixels)
  const { width: w, height: h } = pixels
  const rand = makeRandom(num(seed, 1))
  let field = new Float32Array(w * h)
  for (let j = 0; j < field.length; j++) {
    const u = rand() || 1e-12
    field[j] = Math.sqrt(-2 * Math.log(u)) * Math.cos(2 * Math.PI * rand())
  }
  const sz = Math.max(1, num(size, 1))
  if (sz > 1) {
    field = blurF(field, w, h, sz / 2)
    let sq = 0
    for (let j = 0; j < field.length; j++) sq += field[j] * field[j]
    const sd = Math.sqrt(sq / field.length) || 1
    for (let j = 0; j < field.length; j++) field[j] /= sd
  }
  return mapRgb(pixels, mask, (r, g, b, o, j) => {
    const l = lumaOf(r, g, b) / 255
    const n = field[j] * amt * 48 * (0.5 + 2 * l * (1 - l))
    o[0] = clamp255(r + n)
    o[1] = clamp255(g + n)
    o[2] = clamp255(b + n)
  })
}

// Separable min filter of radius r over a float field (the dark channel).
function minFilter(f, w, h, r) {
  const tmp = new Float32Array(f.length)
  const out = new Float32Array(f.length)
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      let v = Infinity
      for (let t = Math.max(0, x - r); t <= Math.min(w - 1, x + r); t++) v = Math.min(v, f[y * w + t])
      tmp[y * w + x] = v
    }
  }
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      let v = Infinity
      for (let t = Math.max(0, y - r); t <= Math.min(h - 1, y + r); t++) v = Math.min(v, tmp[t * w + x])
      out[y * w + x] = v
    }
  }
  return out
}

// Camera-raw style develop, all -100..100: temp (blue..yellow), tint (green..magenta), clarity
// (midtone local contrast), dehaze (dark-channel haze removal; negative adds haze), texture (fine detail).
export function develop(pixels, { temp = 0, tint = 0, clarity = 0, dehaze = 0, texture = 0 } = {}, mask = null) {
  const T = clamp(num(temp, 0), -100, 100) / 100
  const Ti = clamp(num(tint, 0), -100, 100) / 100
  const C = clamp(num(clarity, 0), -100, 100) / 100
  const D = clamp(num(dehaze, 0), -100, 100) / 100
  const X = clamp(num(texture, 0), -100, 100) / 100
  const { width: w, height: h } = pixels
  const n = w * h
  const cur = clonePixels(pixels)
  const d = cur.data
  if (T || Ti) {
    const gr = 1 + T * 0.2
    const gb = 1 - T * 0.2
    const gg = 1 - Ti * 0.15
    for (let i = 0; i < d.length; i += 4) {
      d[i] = clamp255(d[i] * gr)
      d[i + 1] = clamp255(d[i + 1] * gg)
      d[i + 2] = clamp255(d[i + 2] * gb)
    }
  }
  if (D) {
    const dark = new Float32Array(n)
    for (let j = 0; j < n; j++) dark[j] = Math.min(d[j * 4], d[j * 4 + 1], d[j * 4 + 2])
    const r = Math.max(1, Math.round(Math.min(w, h) / 50))
    const dc = blurF(minFilter(dark, w, h, r), w, h, r)
    // airlight: the mean colour of the haziest 0.1%
    const hist = new Uint32Array(256)
    for (let j = 0; j < n; j++) hist[Math.round(clamp255(dc[j]))]++
    let cut = 255
    for (let acc = 0, want = Math.max(1, Math.ceil(n * 0.001)); cut > 0; cut--) if ((acc += hist[cut]) >= want) break
    const Al = [0, 0, 0]
    let na = 0
    for (let j = 0; j < n; j++) {
      if (Math.round(clamp255(dc[j])) < cut) continue
      for (let c = 0; c < 3; c++) Al[c] += d[j * 4 + c]
      na++
    }
    for (let c = 0; c < 3; c++) Al[c] /= na || 1
    const aMin = Math.max(1, Math.min(...Al))
    for (let j = 0; j < n; j++) {
      const i = j * 4
      for (let c = 0; c < 3; c++) {
        const v = d[i + c]
        if (D > 0) {
          const t = Math.max(0.15, 1 - D * 0.95 * (dc[j] / aMin))
          d[i + c] = clamp255((v - Al[c]) / t + Al[c])
        } else d[i + c] = clamp255(v + (Al[c] - v) * -D * 0.5)
      }
    }
  }
  const local = (sigma, k, midOnly) => {
    const L = new Float32Array(n)
    for (let j = 0; j < n; j++) L[j] = lumaOf(d[j * 4], d[j * 4 + 1], d[j * 4 + 2])
    const Lb = blurF(L, w, h, sigma)
    for (let j = 0; j < n; j++) {
      const l = L[j] / 255
      const wt = midOnly ? 1 - (2 * l - 1) * (2 * l - 1) : 1
      const add = (L[j] - Lb[j]) * k * wt
      if (!add) continue
      d[j * 4] = clamp255(d[j * 4] + add)
      d[j * 4 + 1] = clamp255(d[j * 4 + 1] + add)
      d[j * 4 + 2] = clamp255(d[j * 4 + 2] + add)
    }
  }
  if (C) local(Math.max(2, Math.min(w, h) / 40), C * 1.2, true)
  if (X) local(1.5, X * 0.9, false)
  return mixByMask(pixels, cur, mask)
}

const FILTER_FNS = { motionBlur, radialBlur, surfaceBlur, lensBlur, highPass, pixelate, emboss, findEdges, oilPaint, addGrain, develop }

export const FILTERS = {
  motionBlur: { label: 'Motion Blur', params: { angle: 0, distance: 10 } },
  radialBlur: { label: 'Radial Blur', params: { mode: 'spin', center: null, amount: 10 } },
  surfaceBlur: { label: 'Surface Blur', params: { radius: 5, threshold: 15 } },
  lensBlur: { label: 'Lens Blur', params: { depthMask: null, radius: 10, focus: 0 } },
  highPass: { label: 'High Pass', params: { radius: 10 } },
  pixelate: { label: 'Pixelate', params: { cell: 8 } },
  emboss: { label: 'Emboss', params: { angle: 135, height: 3, amount: 100 } },
  findEdges: { label: 'Find Edges', params: {} },
  oilPaint: { label: 'Oil Paint', params: { radius: 3, levels: 20 } },
  addGrain: { label: 'Add Grain', params: { amount: 20, size: 1, seed: 1 } },
  develop: { label: 'Develop', params: { temp: 0, tint: 0, clarity: 0, dehaze: 0, texture: 0 } },
}

export function applyFilter(pixels, filter, mask = null) {
  const fn = FILTER_FNS[filter?.type]
  return fn ? fn(pixels, filter.params || {}, mask) : clonePixels(pixels)
}

// ---------- 6. warps ----------

// A liquify mesh: nodes every `cell` px, each holding where its output pixel reads from (an offset).
export function liquifyMesh(width, height, cell = 16) {
  const c = Math.max(2, Math.round(num(cell, 16)))
  const cols = Math.ceil(width / c) + 1
  const rows = Math.ceil(height / c) + 1
  return { width, height, cell: c, cols, rows, dx: new Float32Array(cols * rows), dy: new Float32Array(cols * rows) }
}

// The mesh offset at any (x, y), bilinear between nodes.
function meshAt(mesh, dxs, dys, x, y, out) {
  const { cell, cols, rows } = mesh
  const gx = clamp(x / cell, 0, cols - 1)
  const gy = clamp(y / cell, 0, rows - 1)
  const x0 = Math.min(Math.floor(gx), cols - 2 < 0 ? 0 : cols - 2)
  const y0 = Math.min(Math.floor(gy), rows - 2 < 0 ? 0 : rows - 2)
  const x1 = Math.min(cols - 1, x0 + 1)
  const y1 = Math.min(rows - 1, y0 + 1)
  const tx = gx - x0
  const ty = gy - y0
  const a = y0 * cols + x0
  const b = y0 * cols + x1
  const c = y1 * cols + x0
  const d = y1 * cols + x1
  out[0] = (dxs[a] * (1 - tx) + dxs[b] * tx) * (1 - ty) + (dxs[c] * (1 - tx) + dxs[d] * tx) * ty
  out[1] = (dys[a] * (1 - tx) + dys[b] * tx) * (1 - ty) + (dys[c] * (1 - tx) + dys[d] * tx) * ty
  return out
}

// One liquify stroke. push drags content along the stroke; bloat swells it from each point; pucker
// pinches it in; reconstruct relaxes the mesh back. size = brush diameter, pressure 0..1.
export function liquifyStroke(mesh, { mode = 'push', points = [], size = 60, pressure = 0.5 } = {}) {
  const out = { ...mesh, dx: Float32Array.from(mesh.dx), dy: Float32Array.from(mesh.dy) }
  const pts = (points || []).map(toPt)
  if (!pts.length) return out
  const r = Math.max(1, num(size, 60) / 2)
  const p = clamp01(num(pressure, 0.5))
  const steps = densify(pts, Math.max(1, r / 4))
  const { cell, cols, rows } = mesh
  const tmp = [0, 0]
  for (let k = 0; k < steps.length; k++) {
    const [px, py] = steps[k]
    if (mode === 'push' && k === 0) continue
    const [qx, qy] = k > 0 ? steps[k - 1] : steps[k]
    const vx = px - qx
    const vy = py - qy
    const prevDx = Float32Array.from(out.dx)
    const prevDy = Float32Array.from(out.dy)
    const i0 = clamp(Math.floor((px - r) / cell), 0, cols - 1)
    const i1 = clamp(Math.ceil((px + r) / cell), 0, cols - 1)
    const j0 = clamp(Math.floor((py - r) / cell), 0, rows - 1)
    const j1 = clamp(Math.ceil((py + r) / cell), 0, rows - 1)
    for (let j = j0; j <= j1; j++) {
      for (let i = i0; i <= i1; i++) {
        const nx = i * cell
        const ny = j * cell
        const dist = Math.hypot(nx - px, ny - py)
        if (dist >= r) continue
        const q = 1 - (dist / r) * (dist / r)
        const s = p * q * q
        const idx = j * cols + i
        if (mode === 'reconstruct') {
          out.dx[idx] = prevDx[idx] * (1 - s * 0.3)
          out.dy[idx] = prevDy[idx] * (1 - s * 0.3)
          continue
        }
        let ox = 0
        let oy = 0
        if (mode === 'push') {
          ox = -vx * s
          oy = -vy * s
        } else {
          const rate = s * 0.08 * (mode === 'pucker' ? 1 : -1)
          ox = (nx - px) * rate
          oy = (ny - py) * rate
        }
        // the node now reads from where the old field read at its moved spot
        meshAt(mesh, prevDx, prevDy, nx + ox, ny + oy, tmp)
        out.dx[idx] = tmp[0] + ox
        out.dy[idx] = tmp[1] + oy
      }
    }
  }
  return out
}

// Renders pixels through a liquify mesh (inverse map, bilinear, edges clamped).
export function applyMesh(pixels, mesh) {
  const { width: w, height: h } = pixels
  const f = premul(pixels)
  const out = makePixels(w, h)
  const acc = [0, 0, 0, 0]
  const off = [0, 0]
  const sx = mesh.width ? mesh.width / w : 1
  const sy = mesh.height ? mesh.height / h : 1
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      meshAt(mesh, mesh.dx, mesh.dy, x * sx, y * sy, off)
      acc[0] = acc[1] = acc[2] = acc[3] = 0
      samplePremul(f, w, h, x + off[0] / sx, y + off[1] / sy, acc)
      writeAcc(out.data, (y * w + x) * 4, acc)
    }
  }
  return out
}

// The 16 control points of an untouched warp over width x height, row by row.
export function defaultWarpGrid(width, height) {
  const g = []
  for (let r = 0; r < 4; r++) for (let c = 0; c < 4; c++) g.push([(c / 3) * width, (r / 3) * height])
  return g
}

const bern = (t) => [(1 - t) ** 3, 3 * t * (1 - t) ** 2, 3 * t * t * (1 - t), t ** 3]

// Every output pixel centre inside triangle a-b-c, with its barycentric weights.
function rasterTri(a, b, c, W, H, visit) {
  const area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
  if (Math.abs(area) < 1e-9) return
  const x0 = clamp(Math.floor(Math.min(a[0], b[0], c[0])), 0, W)
  const x1 = clamp(Math.ceil(Math.max(a[0], b[0], c[0])), 0, W)
  const y0 = clamp(Math.floor(Math.min(a[1], b[1], c[1])), 0, H)
  const y1 = clamp(Math.ceil(Math.max(a[1], b[1], c[1])), 0, H)
  for (let py = y0; py < y1; py++) {
    const y = py + 0.5
    for (let px = x0; px < x1; px++) {
      const x = px + 0.5
      const w0 = ((b[0] - x) * (c[1] - y) - (b[1] - y) * (c[0] - x)) / area
      const w1 = ((c[0] - x) * (a[1] - y) - (c[1] - y) * (a[0] - x)) / area
      const w2 = 1 - w0 - w1
      if (w0 >= -1e-6 && w1 >= -1e-6 && w2 >= -1e-6) visit(px, py, w0, w1, w2)
    }
  }
}

// Photoshop's Warp: grid is 16 control points (row by row, or 4 rows of 4) of a bicubic Bezier patch
// over the picture; defaultWarpGrid gives the identity. Outside the warped shape is transparent.
export function meshWarp(pixels, grid, { width, height } = {}) {
  const { width: sw, height: sh } = pixels
  const W = Math.max(1, Math.round(width ?? sw))
  const H = Math.max(1, Math.round(height ?? sh))
  const P = (Array.isArray(grid?.[0]?.[0]) ? grid.flat() : grid || []).map(toPt)
  if (P.length !== 16) throw new Error('meshWarp needs 16 control points')
  const N = clamp(Math.round(Math.max(W, H) / 8), 8, 64)
  const S = []
  for (let vi = 0; vi <= N; vi++) {
    const bv = bern(vi / N)
    for (let ui = 0; ui <= N; ui++) {
      const bu = bern(ui / N)
      let x = 0
      let y = 0
      for (let r = 0; r < 4; r++) {
        for (let c = 0; c < 4; c++) {
          const k = bv[r] * bu[c]
          x += P[r * 4 + c][0] * k
          y += P[r * 4 + c][1] * k
        }
      }
      S.push([x, y, ui / N, vi / N])
    }
  }
  const f = premul(pixels)
  const out = makePixels(W, H)
  const acc = [0, 0, 0, 0]
  const put = (a, b, c) =>
    rasterTri(a, b, c, W, H, (px, py, w0, w1, w2) => {
      const u = a[2] * w0 + b[2] * w1 + c[2] * w2
      const v = a[3] * w0 + b[3] * w1 + c[3] * w2
      acc[0] = acc[1] = acc[2] = acc[3] = 0
      samplePremul(f, sw, sh, u * sw - 0.5, v * sh - 0.5, acc)
      writeAcc(out.data, (py * W + px) * 4, acc)
    })
  for (let vi = 0; vi < N; vi++) {
    for (let ui = 0; ui < N; ui++) {
      const a = S[vi * (N + 1) + ui]
      const b = S[vi * (N + 1) + ui + 1]
      const c = S[(vi + 1) * (N + 1) + ui]
      const d = S[(vi + 1) * (N + 1) + ui + 1]
      put(a, b, d)
      put(a, d, c)
    }
  }
  return out
}

// The 3x3 homography (row-major, last entry 1) taking four src points to four dst points.
export function homography(src, dst) {
  const A = []
  for (let i = 0; i < 4; i++) {
    const [x, y] = toPt(src[i])
    const [X, Y] = toPt(dst[i])
    A.push([x, y, 1, 0, 0, 0, -X * x, -X * y, X])
    A.push([0, 0, 0, x, y, 1, -Y * x, -Y * y, Y])
  }
  for (let c = 0; c < 8; c++) {
    let piv = c
    for (let r = c + 1; r < 8; r++) if (Math.abs(A[r][c]) > Math.abs(A[piv][c])) piv = r
    if (Math.abs(A[piv][c]) < 1e-12) return null
    ;[A[c], A[piv]] = [A[piv], A[c]]
    for (let r = 0; r < 8; r++) {
      if (r === c) continue
      const k = A[r][c] / A[c][c]
      for (let k2 = c; k2 < 9; k2++) A[r][k2] -= k * A[c][k2]
    }
  }
  return [...A.map((row, i) => row[8] / row[i]), 1]
}

export function applyHomography(Hm, x, y) {
  const w = Hm[6] * x + Hm[7] * y + Hm[8]
  return [(Hm[0] * x + Hm[1] * y + Hm[2]) / w, (Hm[3] * x + Hm[4] * y + Hm[5]) / w]
}

// Pins the picture's corners (top-left, top-right, bottom-right, bottom-left) to corners4.
export function perspective(pixels, corners, { width, height } = {}) {
  const { width: sw, height: sh } = pixels
  const W = Math.max(1, Math.round(width ?? sw))
  const H = Math.max(1, Math.round(height ?? sh))
  const out = makePixels(W, H)
  const inv = homography(corners, [
    [0, 0],
    [sw, 0],
    [sw, sh],
    [0, sh],
  ])
  if (!inv) return out
  const f = premul(pixels)
  const acc = [0, 0, 0, 0]
  for (let y = 0; y < H; y++) {
    for (let x = 0; x < W; x++) {
      const [u, v] = applyHomography(inv, x + 0.5, y + 0.5)
      if (!(u >= 0 && v >= 0 && u <= sw && v <= sh)) continue
      acc[0] = acc[1] = acc[2] = acc[3] = 0
      samplePremul(f, sw, sh, u - 0.5, v - 0.5, acc)
      writeAcc(out.data, (y * W + x) * 4, acc)
    }
  }
  return out
}

// ---------- 7. retouch brushes ----------

// How much a stroke covers each pixel (strongest touch wins, never builds up), and its box.
function brushCover(w, h, { points = [], size = 40, hardness = 0.5 } = {}, mask = null) {
  const r = Math.max(0.5, num(size, 40) / 2)
  const hard = clamp01(num(hardness, 0.5))
  const pts = densify((points || []).map(toPt), Math.max(1, r / 3))
  const cover = new Float32Array(w * h)
  let X0 = w
  let Y0 = h
  let X1 = 0
  let Y1 = 0
  for (const [px, py] of pts) {
    const x0 = clamp(Math.floor(px - r), 0, w)
    const x1 = clamp(Math.ceil(px + r), 0, w)
    const y0 = clamp(Math.floor(py - r), 0, h)
    const y1 = clamp(Math.ceil(py + r), 0, h)
    if (x1 <= x0 || y1 <= y0) continue
    for (let y = y0; y < y1; y++) {
      for (let x = x0; x < x1; x++) {
        const c = brushFalloff(Math.hypot(x + 0.5 - px, y + 0.5 - py), r, hard)
        if (c > cover[y * w + x]) cover[y * w + x] = c
      }
    }
    X0 = Math.min(X0, x0)
    Y0 = Math.min(Y0, y0)
    X1 = Math.max(X1, x1)
    Y1 = Math.max(Y1, y1)
  }
  if (mask) for (let j = 0; j < cover.length; j++) if (cover[j]) cover[j] *= mask[j] / 255
  return { cover, box: { x: X0, y: Y0, w: Math.max(0, X1 - X0), h: Math.max(0, Y1 - Y0) } }
}

// fn(r, g, b, k, out3) per covered pixel, k = strength * cover.
function brushMap(pixels, opts, mask, fn) {
  const out = clonePixels(pixels)
  const { cover, box } = brushCover(pixels.width, pixels.height, opts, mask)
  const st = clamp01(num(opts?.strength, 0.5))
  const d = out.data
  const o = [0, 0, 0]
  for (let y = box.y; y < box.y + box.h; y++) {
    for (let x = box.x; x < box.x + box.w; x++) {
      const j = y * pixels.width + x
      const k = cover[j] * st
      if (!k) continue
      const i = j * 4
      fn(d[i], d[i + 1], d[i + 2], k, o)
      d[i] = o[0]
      d[i + 1] = o[1]
      d[i + 2] = o[2]
    }
  }
  return out
}

// make(crop) works on the stroke's box (plus a margin) and is mixed in by the stroke's cover.
function brushImage(pixels, opts, mask, margin, make) {
  const out = clonePixels(pixels)
  const { width: w, height: h } = pixels
  const { cover, box } = brushCover(w, h, opts, mask)
  if (!box.w || !box.h) return out
  const st = clamp01(num(opts?.strength, 0.5))
  const rx = Math.max(0, box.x - margin)
  const ry = Math.max(0, box.y - margin)
  const crop = cropPixels(pixels, { x: rx, y: ry, w: Math.min(w, box.x + box.w + margin) - rx, h: Math.min(h, box.y + box.h + margin) - ry })
  const done = make(crop).data
  const d = out.data
  for (let y = box.y; y < box.y + box.h; y++) {
    for (let x = box.x; x < box.x + box.w; x++) {
      const t = cover[y * w + x] * st
      if (!t) continue
      const i = (y * w + x) * 4
      const ci = ((y - ry) * crop.width + (x - rx)) * 4
      for (let c = 0; c < 4; c++) d[i + c] = d[i + c] + (done[ci + c] - d[i + c]) * t
    }
  }
  return out
}

function toneWeight(l, range) {
  if (range === 'shadows') return 1 - smooth(0, 0.6, l)
  if (range === 'highlights') return smooth(0.4, 1, l)
  return 1 - (2 * l - 1) * (2 * l - 1)
}

// Dodge lightens, burn darkens, in the shadows, midtones or highlights.
export function dodge(pixels, opts = {}, mask = null) {
  const range = opts.range || 'midtones'
  return brushMap(pixels, opts, mask, (r, g, b, k, o) => {
    const t = k * toneWeight(lumaOf(r, g, b) / 255, range)
    o[0] = r + (255 - r) * t
    o[1] = g + (255 - g) * t
    o[2] = b + (255 - b) * t
  })
}

export function burn(pixels, opts = {}, mask = null) {
  const range = opts.range || 'midtones'
  return brushMap(pixels, opts, mask, (r, g, b, k, o) => {
    const t = k * toneWeight(lumaOf(r, g, b) / 255, range)
    o[0] = r * (1 - t)
    o[1] = g * (1 - t)
    o[2] = b * (1 - t)
  })
}

// mode 'saturate' or 'desaturate'.
export function sponge(pixels, opts = {}, mask = null) {
  const sign = opts.mode === 'desaturate' ? -1 : 1
  return brushMap(pixels, opts, mask, (r, g, b, k, o) => {
    const L = lumaOf(r, g, b)
    const f = Math.max(0, 1 + sign * k)
    o[0] = clamp255(L + (r - L) * f)
    o[1] = clamp255(L + (g - L) * f)
    o[2] = clamp255(L + (b - L) * f)
  })
}

// Drags colour along the stroke like a finger in wet paint; strength is how much the finger keeps.
export function smudge(pixels, { points = [], size = 30, hardness = 0.5, strength = 0.5 } = {}, mask = null) {
  const out = clonePixels(pixels)
  const pts = (points || []).map(toPt)
  if (pts.length < 2) return out
  const { width: w, height: h } = pixels
  const d = out.data
  const r = Math.max(1, num(size, 30) / 2)
  const D = Math.ceil(r) * 2 + 1
  const half = (D - 1) / 2
  const st = clamp01(num(strength, 0.5))
  const hard = clamp01(num(hardness, 0.5))
  const finger = new Float32Array(D * D * 4)
  const grab = (cx, cy, into, keep) => {
    for (let oy = 0; oy < D; oy++) {
      for (let ox = 0; ox < D; ox++) {
        const x = clamp(Math.round(cx) + ox - half, 0, w - 1)
        const y = clamp(Math.round(cy) + oy - half, 0, h - 1)
        const i = (y * w + x) * 4
        const q = (oy * D + ox) * 4
        for (let c = 0; c < 4; c++) into[q + c] = into[q + c] * keep + d[i + c] * (1 - keep)
      }
    }
  }
  const steps = densify(pts, Math.max(1, r / 4))
  grab(steps[0][0], steps[0][1], finger, 0)
  for (let k = 1; k < steps.length; k++) {
    const [cx, cy] = steps[k]
    const bx = Math.round(cx) - half
    const by = Math.round(cy) - half
    for (let oy = 0; oy < D; oy++) {
      for (let ox = 0; ox < D; ox++) {
        const x = bx + ox
        const y = by + oy
        if (x < 0 || y < 0 || x >= w || y >= h) continue
        let cov = brushFalloff(Math.hypot(ox - half, oy - half), r, hard) * st
        if (mask) cov *= mask[y * w + x] / 255
        if (!cov) continue
        const i = (y * w + x) * 4
        const q = (oy * D + ox) * 4
        for (let c = 0; c < 4; c++) d[i + c] = d[i + c] + (finger[q + c] - d[i + c]) * cov
      }
    }
    grab(cx, cy, finger, st)
  }
  return out
}

export function blurBrush(pixels, opts = {}, mask = null) {
  const sigma = Math.max(0.8, num(opts.size, 30) / 12)
  return brushImage(pixels, opts, mask, Math.ceil(sigma * 3), (crop) => gaussianBlur(crop, sigma))
}

export function sharpenBrush(pixels, opts = {}, mask = null) {
  return brushImage(pixels, { ...opts, strength: 1 }, mask, 3, (crop) => unsharp(crop, { amount: 200 * clamp01(num(opts.strength, 0.5)), radius: 1 }))
}

// Spot healing: finds the nearby patch whose surroundings best match the stroke's (zero-mean, so
// texture counts more than tone), then heals from it with engine2's colour-matched heal.
export function spotHeal(pixels, { points = [], size = 20, hardness = 0.6 } = {}, mask = null) {
  const pts = (points || []).map(toPt)
  if (!pts.length) return clonePixels(pixels)
  const { width: w, height: h, data: s } = pixels
  const r = Math.max(1, num(size, 20) / 2)
  const ring = Math.max(2, Math.round(r / 2))
  const dense = densify(pts, Math.max(1, r / 3))
  const xs = dense.map((p) => p[0])
  const ys = dense.map((p) => p[1])
  const X0 = clamp(Math.floor(Math.min(...xs) - r - ring), 0, w)
  const X1 = clamp(Math.ceil(Math.max(...xs) + r + ring), 0, w)
  const Y0 = clamp(Math.floor(Math.min(...ys) - r - ring), 0, h)
  const Y1 = clamp(Math.ceil(Math.max(...ys) + r + ring), 0, h)
  const ringPx = []
  for (let y = Y0; y < Y1; y++) {
    for (let x = X0; x < X1; x++) {
      let dmin = Infinity
      for (const [px, py] of dense) dmin = Math.min(dmin, Math.hypot(x + 0.5 - px, y + 0.5 - py))
      if (dmin >= r && dmin <= r + ring) ringPx.push([x, y])
    }
  }
  const step = Math.max(1, Math.floor(ringPx.length / 400))
  const sample = ringPx.filter((_, i) => i % step === 0)
  const ext = Math.max(X1 - X0, Y1 - Y0)
  let best = null
  let bestScore = Infinity
  const score = (dx, dy) => {
    const mD = [0, 0, 0]
    const mS = [0, 0, 0]
    for (const [x, y] of sample) {
      const i = (y * w + x) * 4
      const j = ((y + dy) * w + (x + dx)) * 4
      for (let c = 0; c < 3; c++) {
        mD[c] += s[i + c]
        mS[c] += s[j + c]
      }
    }
    for (let c = 0; c < 3; c++) {
      mD[c] /= sample.length || 1
      mS[c] /= sample.length || 1
    }
    let e = 0
    for (const [x, y] of sample) {
      const i = (y * w + x) * 4
      const j = ((y + dy) * w + (x + dx)) * 4
      for (let c = 0; c < 3; c++) e += (s[i + c] - mD[c] - (s[j + c] - mS[c])) ** 2
    }
    return e
  }
  for (const f of [1, 1.4, 2]) {
    for (let a = 0; a < 16; a++) {
      const ang = (a * Math.PI) / 8
      const dx = Math.round(Math.cos(ang) * ext * f)
      const dy = Math.round(Math.sin(ang) * ext * f)
      if (X0 + dx < 0 || X1 + dx > w || Y0 + dy < 0 || Y1 + dy > h) continue
      const e = score(dx, dy)
      if (e < bestScore) {
        bestScore = e
        best = [dx, dy]
      }
    }
  }
  if (!best) {
    // no room for a clean patch: take the side with the most space
    const room = [
      [w - X1, Math.min(ext, w - X1), 0],
      [X0, -Math.min(ext, X0), 0],
      [h - Y1, 0, Math.min(ext, h - Y1)],
      [Y0, 0, -Math.min(ext, Y0)],
    ].sort((a, b) => b[0] - a[0])[0]
    best = room[1] || room[2] ? [room[1], room[2]] : [ext, 0]
  }
  return heal(pixels, { source: [pts[0][0] + best[0], pts[0][1] + best[1]], points: pts, size: num(size, 20), hardness: num(hardness, 0.6) }, mask)
}

// Patch tool: the selected area (mask) is rebuilt from the area dx, dy away, blended seamlessly
// (its texture kept, its tone solved to meet the border: a membrane / Poisson fill).
export function patch(pixels, mask, { dx = 0, dy = 0, iterations = null } = {}) {
  const out = clonePixels(pixels)
  if (!mask) return out
  const { width: w, height: h, data: s } = pixels
  let X0 = w
  let Y0 = h
  let X1 = -1
  let Y1 = -1
  for (let j = 0; j < mask.length; j++) {
    if (!mask[j]) continue
    const x = j % w
    const y = (j - x) / w
    X0 = Math.min(X0, x)
    X1 = Math.max(X1, x)
    Y0 = Math.min(Y0, y)
    Y1 = Math.max(Y1, y)
  }
  if (X1 < 0) return out
  const ox = Math.round(num(dx, 0))
  const oy = Math.round(num(dy, 0))
  X0 = Math.max(0, X0 - 1)
  Y0 = Math.max(0, Y0 - 1)
  X1 = Math.min(w - 1, X1 + 1)
  Y1 = Math.min(h - 1, Y1 + 1)
  const bw = X1 - X0 + 1
  const bh = Y1 - Y0 + 1
  const src = (x, y, c) => s[(clamp(y + oy, 0, h - 1) * w + clamp(x + ox, 0, w - 1)) * 4 + c]
  const inside = new Uint8Array(bw * bh)
  const corr = new Float32Array(bw * bh * 3)
  const mean = [0, 0, 0]
  let nb = 0
  for (let y = Y0; y <= Y1; y++) {
    for (let x = X0; x <= X1; x++) {
      const q = (y - Y0) * bw + (x - X0)
      if (mask[y * w + x]) inside[q] = 1
      else {
        for (let c = 0; c < 3; c++) {
          corr[q * 3 + c] = s[(y * w + x) * 4 + c] - src(x, y, c)
          mean[c] += corr[q * 3 + c]
        }
        nb++
      }
    }
  }
  for (let q = 0; q < inside.length; q++) if (inside[q]) for (let c = 0; c < 3; c++) corr[q * 3 + c] = nb ? mean[c] / nb : 0
  const iters = iterations ?? clamp(Math.round(2 * Math.max(bw, bh)), 50, 400)
  for (let it = 0; it < iters; it++) {
    for (let ly = 0; ly < bh; ly++) {
      for (let lx = 0; lx < bw; lx++) {
        const q = ly * bw + lx
        if (!inside[q]) continue
        for (let c = 0; c < 3; c++) {
          let sum = 0
          let cnt = 0
          if (lx > 0) {
            sum += corr[(q - 1) * 3 + c]
            cnt++
          }
          if (lx < bw - 1) {
            sum += corr[(q + 1) * 3 + c]
            cnt++
          }
          if (ly > 0) {
            sum += corr[(q - bw) * 3 + c]
            cnt++
          }
          if (ly < bh - 1) {
            sum += corr[(q + bw) * 3 + c]
            cnt++
          }
          if (cnt) corr[q * 3 + c] += 1.9 * (sum / cnt - corr[q * 3 + c])
        }
      }
    }
  }
  const d = out.data
  for (let y = Y0; y <= Y1; y++) {
    for (let x = X0; x <= X1; x++) {
      const j = y * w + x
      if (!mask[j]) continue
      const q = (y - Y0) * bw + (x - X0)
      const t = mask[j] / 255
      for (let c = 0; c < 3; c++) {
        const v = clamp255(src(x, y, c) + corr[q * 3 + c])
        d[j * 4 + c] = s[j * 4 + c] + (v - s[j * 4 + c]) * t
      }
    }
  }
  return out
}

// ---------- 8. selections ----------

// Quick selection: strokes are lists of points (or { points, size }). A region grows from them by
// colour (near a small palette of the stroked colours, no sharp steps), then GrabCut refines it with
// the strokes as sure foreground. add adds to prevMask, add: false takes it away.
export function quickSelect(pixels, strokes = [], { add = true, prevMask = null, size = 20, tolerance = 32, refine = true } = {}) {
  const { width: w, height: h, data: d } = pixels
  const n = w * h
  let list = Array.isArray(strokes) ? strokes : [strokes]
  if (list.length && (typeof list[0]?.[0] === 'number' || typeof list[0]?.x === 'number')) list = [list]
  list = list.map((s) => (Array.isArray(s) ? { points: s, size } : { points: s?.points || [], size: s?.size ?? size }))
  const seed = new Uint8Array(n)
  const seedPts = []
  for (const s of list) {
    const { cover } = brushCover(w, h, { points: s.points, size: s.size, hardness: 1 })
    for (let j = 0; j < n; j++) if (cover[j] > 0.5) seed[j] = 1
    for (const p of s.points) seedPts.push(toPt(p))
  }
  const finish = (m) => {
    if (!prevMask) return add ? m : new Uint8Array(n)
    return combineMasks(prevMask, m, add ? 'add' : 'subtract')
  }
  const seeds = []
  for (let j = 0; j < n; j++) if (seed[j]) seeds.push(j)
  if (!seeds.length) return finish(new Uint8Array(n))
  const tol = Math.max(1, num(tolerance, 32))
  const dist = (i, k) => Math.sqrt(((d[i] - d[k]) ** 2 + (d[i + 1] - d[k + 1]) ** 2 + (d[i + 2] - d[k + 2]) ** 2) / 3)
  const distC = (i, c) => Math.sqrt(((d[i] - c[0]) ** 2 + (d[i + 1] - c[1]) ** 2 + (d[i + 2] - c[2]) ** 2) / 3)
  // a palette of up to 8 stroked colours, each far from the others
  const sampleStep = Math.max(1, Math.floor(seeds.length / 400))
  const samples = seeds.filter((_, i) => i % sampleStep === 0).map((j) => [d[j * 4], d[j * 4 + 1], d[j * 4 + 2]])
  const palette = [samples[0]]
  while (palette.length < 8) {
    let far = null
    let farD = tol / 2
    for (const c of samples) {
      const dd = Math.min(...palette.map((p) => Math.sqrt(((p[0] - c[0]) ** 2 + (p[1] - c[1]) ** 2 + (p[2] - c[2]) ** 2) / 3)))
      if (dd > farD) {
        farD = dd
        far = c
      }
    }
    if (!far) break
    palette.push(far)
  }
  const region = new Uint8Array(n)
  const queue = new Int32Array(n)
  let head = 0
  let tail = 0
  for (const j of seeds) {
    region[j] = 1
    queue[tail++] = j
  }
  while (head < tail) {
    const p = queue[head++]
    const px = p % w
    const py = (p - px) / w
    for (const [nx, ny] of [
      [px + 1, py],
      [px - 1, py],
      [px, py + 1],
      [px, py - 1],
    ]) {
      if (nx < 0 || ny < 0 || nx >= w || ny >= h) continue
      const q = ny * w + nx
      if (region[q]) continue
      if (dist(p * 4, q * 4) > tol * 1.5) continue
      let near = Infinity
      for (const c of palette) near = Math.min(near, distC(q * 4, c))
      if (near > tol) continue
      region[q] = 1
      queue[tail++] = q
    }
  }
  let grown = Uint8Array.from(region, (v) => (v ? 255 : 0))
  if (refine) {
    let x0 = w
    let y0 = h
    let x1 = -1
    let y1 = -1
    for (let j = 0; j < n; j++) {
      if (!region[j]) continue
      const x = j % w
      const y = (j - x) / w
      x0 = Math.min(x0, x)
      x1 = Math.max(x1, x)
      y0 = Math.min(y0, y)
      y1 = Math.max(y1, y)
    }
    const padX = Math.max(2, Math.round((x1 - x0 + 1) * 0.1))
    const padY = Math.max(2, Math.round((y1 - y0 + 1) * 0.1))
    const rect = { x: Math.max(0, x0 - padX), y: Math.max(0, y0 - padY), w: 0, h: 0 }
    rect.w = Math.min(w, x1 + 1 + padX) - rect.x
    rect.h = Math.min(h, y1 + 1 + padY) - rect.y
    // GrabCut needs some background inside its frame; skip it when the region fills the picture
    if (rect.w < w || rect.h < h) {
      const g = grabCut(pixels, rect, { hints: { fg: seedPts } })
      let hit = 0
      for (const j of seeds) if (g[j] > 127) hit++
      let area = 0
      let areaG = 0
      for (let j = 0; j < n; j++) {
        if (region[j]) area++
        if (g[j] > 127) areaG++
      }
      if (hit >= seeds.length * 0.6 && areaG >= area * 0.5 && areaG <= area * 2) grown = combineMasks(g, Uint8Array.from(seed, (v) => (v ? 255 : 0)), 'add')
    }
  }
  return finish(grown)
}

// Object selection: GrabCut inside the rectangle { x, y, w, h }, then smoothed and lightly feathered.
export function objectSelect(pixels, rect, { feather = 0.8, smooth: sm = 2 } = {}) {
  const m = grabCut(pixels, rect, { iterations: 4 })
  return refineMask(m, pixels.width, pixels.height, { smooth: sm, feather })
}

// ---------- 9. guides, snapping and artboards ----------

// The nearest target within threshold. targets are numbers or { value, ... }.
// Returns { value (snapped or unchanged), snapped, target (the matched entry or null) }.
export function snapTo(value, targets = [], threshold = 6) {
  let best = null
  let bestD = Infinity
  for (const t of targets || []) {
    const tv = typeof t === 'number' ? t : Number(t?.value)
    if (!Number.isFinite(tv)) continue
    const dd = Math.abs(value - tv)
    if (dd <= threshold && dd < bestD) {
      bestD = dd
      best = t
    }
  }
  if (best == null) return { value, snapped: false, target: null }
  return { value: typeof best === 'number' ? best : Number(best.value), snapped: true, target: best }
}

// Snaps a box's left / centre / right edge to xs and its top / middle / bottom to ys (whichever is
// closest). Returns { x, y, snapX: target | null, snapY: target | null }.
export function snapBox({ x, y, w, h }, { xs = [], ys = [] } = {}, threshold = 6) {
  const axis = (pos, size, targets) => {
    let best = { value: pos, snapped: false, target: null }
    let bestD = Infinity
    for (const k of [0, 0.5, 1]) {
      const r = snapTo(pos + size * k, targets, threshold)
      if (!r.snapped) continue
      const dd = Math.abs(r.value - (pos + size * k))
      if (dd < bestD) {
        bestD = dd
        best = { value: r.value - size * k, snapped: true, target: r.target }
      }
    }
    return best
  }
  const sx = axis(x, w, xs)
  const sy = axis(y, h, ys)
  return { x: sx.value, y: sy.value, snapX: sx.target, snapY: sy.target }
}

export const artboardPresets = [
  { id: 'icon-1024', group: 'App icon', name: 'App icon 1024', width: 1024, height: 1024 },
  { id: 'icon-512', group: 'App icon', name: 'App icon 512', width: 512, height: 512 },
  { id: 'icon-192', group: 'App icon', name: 'Android icon 192', width: 192, height: 192 },
  { id: 'icon-180', group: 'App icon', name: 'iOS icon 180', width: 180, height: 180 },
  { id: 'icon-48', group: 'App icon', name: 'Icon 48', width: 48, height: 48 },
  { id: 'social-og', group: 'Social', name: 'Link preview 1200 x 630', width: 1200, height: 630 },
  { id: 'social-square', group: 'Social', name: 'Square post 1080 x 1080', width: 1080, height: 1080 },
  { id: 'hd', group: 'Screen', name: 'Full HD 1920 x 1080', width: 1920, height: 1080 },
]

// Fits a srcW x srcH picture into w x h: contain shows it all, cover fills the box. Centred.
export function fitInto(srcW, srcH, w, h, mode = 'contain') {
  const sw = Math.max(1e-9, srcW)
  const sh = Math.max(1e-9, srcH)
  const scale = mode === 'cover' ? Math.max(w / sw, h / sh) : Math.min(w / sw, h / sh)
  const width = sw * scale
  const height = sh * scale
  return { x: (w - width) / 2, y: (h - height) / 2, width, height, scale }
}
