// PHOTO's engine: the document model and all the pixel math, kept free of the DOM so it can be
// tested. Pixels are plain { data: Uint8ClampedArray, width, height } objects (RGBA, row by row).

export const BLENDS = ['normal', 'multiply', 'screen', 'overlay', 'darken', 'lighten']
export const ADJUST_KEYS = ['exposure', 'brightness', 'contrast', 'highlights', 'shadows', 'warmth', 'saturation', 'sharpness']

let counter = 0
export function newId(prefix = 'layer') {
  counter += 1
  return `${prefix}-${Date.now().toString(36)}-${counter.toString(36)}${Math.random().toString(36).slice(2, 6)}`
}

const clamp = (v, lo, hi) => (v < lo ? lo : v > hi ? hi : v)
const clamp255 = (v) => (v < 0 ? 0 : v > 255 ? 255 : v)

export function makePixels(width, height, fill) {
  const data = new Uint8ClampedArray(width * height * 4)
  if (fill) for (let i = 0; i < data.length; i += 4) data.set(fill, i)
  return { data, width, height }
}

export function clonePixels(p) {
  return { data: new Uint8ClampedArray(p.data), width: p.width, height: p.height }
}

// ---------- the document ----------

export function emptyAdjust() {
  return Object.fromEntries(ADJUST_KEYS.map((k) => [k, 0]))
}

export function isAdjustEmpty(adjust) {
  return !adjust || ADJUST_KEYS.every((k) => !adjust[k])
}

export function newDoc(width, height) {
  return { width: Math.max(1, Math.round(width)), height: Math.max(1, Math.round(height)), layers: [], adjust: emptyAdjust(), selection: null }
}

// A document read back from disk (or from her) with every field present.
export function normaliseDoc(doc) {
  const width = Math.max(1, Math.round(Number(doc?.width) || 1))
  const height = Math.max(1, Math.round(Number(doc?.height) || 1))
  const layers = (Array.isArray(doc?.layers) ? doc.layers : []).map((l, i) => ({
    id: String(l?.id || newId()),
    name: String(l?.name || `Layer ${i + 1}`),
    kind: l?.kind === 'text' ? 'text' : 'image',
    visible: l?.visible !== false,
    opacity: clamp(Number.isFinite(Number(l?.opacity)) ? Number(l.opacity) : 1, 0, 1),
    blend: BLENDS.includes(l?.blend) ? l.blend : 'normal',
    x: Math.round(Number(l?.x) || 0),
    y: Math.round(Number(l?.y) || 0),
    ...(l?.kind === 'text' ? { text: { ...defaultText(height), ...(l.text || {}) } } : { png: l?.png || '' }),
  }))
  const adjust = emptyAdjust()
  for (const k of ADJUST_KEYS) adjust[k] = clamp(Math.round(Number(doc?.adjust?.[k]) || 0), -100, 100)
  return { width, height, layers, adjust, selection: null }
}

// Puts saved layer pngs (base64, no prefix) back on their layers.
export function restorePixels(doc, layers) {
  const byId = new Map((layers || []).map((l) => [l.id, l.png]))
  return {
    ...doc,
    layers: doc.layers.map((l) => {
      if (l.kind !== 'image') return l
      const png = byId.get(l.id)
      if (!png) return l
      return { ...l, png: png.startsWith('data:') ? png : `data:image/png;base64,${png}` }
    }),
  }
}

export function defaultText(docHeight = 600, color = '#ffffff') {
  return { value: 'Text', font: 'Inter, "Segoe UI", sans-serif', size: Math.max(16, Math.round(docHeight / 12)), color, weight: 600 }
}

function uniqueName(doc, base) {
  const names = new Set(doc.layers.map((l) => l.name))
  if (!names.has(base)) return base
  let n = 2
  while (names.has(`${base} ${n}`)) n += 1
  return `${base} ${n}`
}

// A new image layer goes on top; w and h (its pixel size) centre it in the document.
export function addImageLayer(doc, png, name, w, h) {
  const layer = {
    id: newId(),
    name: uniqueName(doc, name || 'Layer'),
    kind: 'image',
    visible: true,
    opacity: 1,
    blend: 'normal',
    x: w ? Math.round((doc.width - w) / 2) : 0,
    y: h ? Math.round((doc.height - h) / 2) : 0,
    png,
  }
  return { ...doc, layers: [...doc.layers, layer] }
}

export function addTextLayer(doc, value, x = 0, y = 0, text = {}) {
  const layer = {
    id: newId(),
    name: uniqueName(doc, String(value || 'Text').slice(0, 24)),
    kind: 'text',
    visible: true,
    opacity: 1,
    blend: 'normal',
    x: Math.round(x),
    y: Math.round(y),
    text: { ...defaultText(doc.height), ...text, value: String(value ?? '') },
  }
  return { ...doc, layers: [...doc.layers, layer] }
}

// dir +1 moves a layer up (toward the top), -1 down.
export function moveLayer(doc, id, dir) {
  const i = doc.layers.findIndex((l) => l.id === id)
  const j = i + dir
  if (i < 0 || j < 0 || j >= doc.layers.length) return doc
  const layers = [...doc.layers]
  ;[layers[i], layers[j]] = [layers[j], layers[i]]
  return { ...doc, layers }
}

// The copy sits right above its original.
export function duplicateLayer(doc, id) {
  const i = doc.layers.findIndex((l) => l.id === id)
  if (i < 0) return doc
  const src = doc.layers[i]
  const copy = { ...src, id: newId(), name: uniqueName(doc, `${src.name} copy`), ...(src.text ? { text: { ...src.text } } : {}) }
  const layers = [...doc.layers]
  layers.splice(i + 1, 0, copy)
  return { ...doc, layers }
}

// Never the last layer.
export function deleteLayer(doc, id) {
  if (doc.layers.length <= 1 || !doc.layers.some((l) => l.id === id)) return doc
  return { ...doc, layers: doc.layers.filter((l) => l.id !== id) }
}

export function renameLayer(doc, id, name) {
  const clean = String(name || '').trim().slice(0, 60)
  if (!clean) return doc
  return setLayer(doc, id, { name: clean })
}

export function setLayer(doc, id, fields) {
  if (!doc.layers.some((l) => l.id === id)) return doc
  return { ...doc, layers: doc.layers.map((l) => (l.id === id ? { ...l, ...fields } : l)) }
}

// The document without pixel data - what she reads.
export function stripPixels(doc) {
  if (!doc) return doc
  const selection = doc.selection?.kind === 'mask' ? { kind: 'mask' } : doc.selection
  return { ...doc, layers: doc.layers.map(({ png, ...rest }) => rest), selection }
}

// ---------- the document's transforms (the model; layer pixels are transformed separately) ----------

// sizes: { [layerId]: { w, h } } - each layer's own pixel size (text measured).
export function cropDoc(doc, rect) {
  const x = Math.round(rect.x)
  const y = Math.round(rect.y)
  return {
    ...doc,
    width: Math.max(1, Math.round(rect.w)),
    height: Math.max(1, Math.round(rect.h)),
    layers: doc.layers.map((l) => ({ ...l, x: l.x - x, y: l.y - y })),
    selection: null,
  }
}

// turns of 90 degrees clockwise (negative turns go anticlockwise).
export function rotateDoc(doc, turns, sizes = {}) {
  const t = ((turns % 4) + 4) % 4
  if (!t) return doc
  const W = doc.width
  const H = doc.height
  const layers = doc.layers.map((l) => {
    const { w = 0, h = 0 } = sizes[l.id] || {}
    if (t === 1) return { ...l, x: H - (l.y + h), y: l.x }
    if (t === 2) return { ...l, x: W - (l.x + w), y: H - (l.y + h) }
    return { ...l, x: l.y, y: W - (l.x + w) }
  })
  const odd = t % 2 === 1
  return { ...doc, width: odd ? H : W, height: odd ? W : H, layers, selection: null }
}

export function flipDoc(doc, axis, sizes = {}) {
  const layers = doc.layers.map((l) => {
    const { w = 0, h = 0 } = sizes[l.id] || {}
    return axis === 'v' ? { ...l, y: doc.height - (l.y + h) } : { ...l, x: doc.width - (l.x + w) }
  })
  return { ...doc, layers, selection: null }
}

export function resizeDoc(doc, w, h) {
  const width = Math.max(1, Math.round(w))
  const height = Math.max(1, Math.round(h))
  const sx = width / doc.width
  const sy = height / doc.height
  const layers = doc.layers.map((l) => ({
    ...l,
    x: Math.round(l.x * sx),
    y: Math.round(l.y * sy),
    ...(l.text ? { text: { ...l.text, size: Math.max(1, Math.round(l.text.size * ((sx + sy) / 2))) } } : {}),
  }))
  return { ...doc, width, height, layers, selection: null }
}

// ---------- pixel transforms ----------

export function cropPixels(p, rect) {
  const x0 = clamp(Math.round(rect.x), 0, p.width)
  const y0 = clamp(Math.round(rect.y), 0, p.height)
  const x1 = clamp(Math.round(rect.x + rect.w), x0, p.width)
  const y1 = clamp(Math.round(rect.y + rect.h), y0, p.height)
  const out = makePixels(x1 - x0, y1 - y0)
  for (let y = y0; y < y1; y++) out.data.set(p.data.subarray((y * p.width + x0) * 4, (y * p.width + x1) * 4), (y - y0) * out.width * 4)
  return out
}

export function rotatePixels(p, turns) {
  const t = ((turns % 4) + 4) % 4
  if (!t) return clonePixels(p)
  const { width: w, height: h, data } = p
  const out = makePixels(t === 2 ? w : h, t === 2 ? h : w)
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      let nx
      let ny
      if (t === 1) {
        nx = h - 1 - y
        ny = x
      } else if (t === 2) {
        nx = w - 1 - x
        ny = h - 1 - y
      } else {
        nx = y
        ny = w - 1 - x
      }
      const s = (y * w + x) * 4
      const d = (ny * out.width + nx) * 4
      out.data[d] = data[s]
      out.data[d + 1] = data[s + 1]
      out.data[d + 2] = data[s + 2]
      out.data[d + 3] = data[s + 3]
    }
  }
  return out
}

export function flipPixels(p, axis) {
  const { width: w, height: h, data } = p
  const out = makePixels(w, h)
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const sx = axis === 'v' ? x : w - 1 - x
      const sy = axis === 'v' ? h - 1 - y : y
      const s = (sy * w + sx) * 4
      const d = (y * w + x) * 4
      out.data[d] = data[s]
      out.data[d + 1] = data[s + 1]
      out.data[d + 2] = data[s + 2]
      out.data[d + 3] = data[s + 3]
    }
  }
  return out
}

// Bilinear, sampling pixel centres.
export function resizePixels(p, w, h) {
  const W = Math.max(1, Math.round(w))
  const H = Math.max(1, Math.round(h))
  const out = makePixels(W, H)
  const { width: sw, height: sh, data } = p
  const sx = sw / W
  const sy = sh / H
  for (let y = 0; y < H; y++) {
    const fy = clamp((y + 0.5) * sy - 0.5, 0, sh - 1)
    const y0 = Math.floor(fy)
    const y1 = Math.min(y0 + 1, sh - 1)
    const ty = fy - y0
    for (let x = 0; x < W; x++) {
      const fx = clamp((x + 0.5) * sx - 0.5, 0, sw - 1)
      const x0 = Math.floor(fx)
      const x1 = Math.min(x0 + 1, sw - 1)
      const tx = fx - x0
      const a = (y0 * sw + x0) * 4
      const b = (y0 * sw + x1) * 4
      const c = (y1 * sw + x0) * 4
      const d = (y1 * sw + x1) * 4
      const o = (y * W + x) * 4
      for (let k = 0; k < 4; k++) {
        const top = data[a + k] + (data[b + k] - data[a + k]) * tx
        const bottom = data[c + k] + (data[d + k] - data[c + k]) * tx
        out.data[o + k] = top + (bottom - top) * ty
      }
    }
  }
  return out
}

// ---------- blending ----------

// One channel, both 0-1: s is the layer, d the backdrop under it.
export function blendChannel(mode, s, d) {
  switch (mode) {
    case 'multiply':
      return s * d
    case 'screen':
      return s + d - s * d
    case 'overlay':
      return d <= 0.5 ? 2 * s * d : 1 - 2 * (1 - s) * (1 - d)
    case 'darken':
      return Math.min(s, d)
    case 'lighten':
      return Math.max(s, d)
    default:
      return s
  }
}

// src over dst, both [r, g, b, a] in 0-255, with the W3C blend formula; returns [r, g, b, a].
export function blendPixel(mode, src, dst, opacity = 1) {
  const as = (src[3] / 255) * opacity
  const ab = dst[3] / 255
  const ao = as + ab * (1 - as)
  if (ao <= 0) return [0, 0, 0, 0]
  const out = [0, 0, 0, Math.round(ao * 255)]
  for (let k = 0; k < 3; k++) {
    const cs = src[k] / 255
    const cb = dst[k] / 255
    const mixed = (1 - ab) * cs + ab * blendChannel(mode, cs, cb)
    const co = as * mixed + ab * cb * (1 - as)
    out[k] = Math.round(clamp((co / ao) * 255, 0, 255))
  }
  return out
}

// Layers bottom-to-top, all the same size, onto a transparent start.
export function compose(layersPixels, opacities = [], modes = []) {
  const first = layersPixels[0]
  if (!first) return makePixels(1, 1)
  const out = makePixels(first.width, first.height)
  const o = out.data
  const src = [0, 0, 0, 0]
  const dst = [0, 0, 0, 0]
  layersPixels.forEach((layer, n) => {
    const opacity = opacities[n] ?? 1
    const mode = modes[n] || 'normal'
    const s = layer.data
    for (let i = 0; i < o.length; i += 4) {
      if (!s[i + 3]) continue
      src[0] = s[i]
      src[1] = s[i + 1]
      src[2] = s[i + 2]
      src[3] = s[i + 3]
      dst[0] = o[i]
      dst[1] = o[i + 1]
      dst[2] = o[i + 2]
      dst[3] = o[i + 3]
      const r = blendPixel(mode, src, dst, opacity)
      o[i] = r[0]
      o[i + 1] = r[1]
      o[i + 2] = r[2]
      o[i + 3] = r[3]
    }
  })
  return out
}

// ---------- adjustments (non-destructive, on the composite) ----------

const luma = (r, g, b) => 0.299 * r + 0.587 * g + 0.114 * b

export function applyAdjust(pixels, adjust) {
  const out = clonePixels(pixels)
  if (isAdjustEmpty(adjust)) return out
  const a = { ...emptyAdjust(), ...adjust }
  const d = out.data
  const exposure = a.exposure ? Math.pow(2, a.exposure / 100) : 1
  const bright = (a.brightness / 100) * 128
  const contrast = Math.pow((100 + a.contrast) / 100, 2)
  const hi = (a.highlights / 100) * 64
  const sh = (a.shadows / 100) * 64
  const warm = (a.warmth / 100) * 30
  const sat = 1 + a.saturation / 100
  const perPixel = a.exposure || a.brightness || a.contrast || a.highlights || a.shadows || a.warmth || a.saturation
  if (perPixel) {
    for (let i = 0; i < d.length; i += 4) {
      let r = d[i]
      let g = d[i + 1]
      let b = d[i + 2]
      if (a.exposure) {
        r *= exposure
        g *= exposure
        b *= exposure
      }
      if (a.brightness) {
        r += bright
        g += bright
        b += bright
      }
      if (a.contrast) {
        r = (r - 128) * contrast + 128
        g = (g - 128) * contrast + 128
        b = (b - 128) * contrast + 128
      }
      if (a.highlights || a.shadows) {
        // a tone curve on luminance: highlights move the bright half, shadows the dark half
        const L = clamp(luma(r, g, b) / 255, 0, 1)
        const wHi = L > 0.5 ? ((L - 0.5) / 0.5) ** 2 : 0
        const wSh = L < 0.5 ? ((0.5 - L) / 0.5) ** 2 : 0
        const shift = hi * wHi + sh * wSh
        r += shift
        g += shift
        b += shift
      }
      if (a.warmth) {
        r += warm
        b -= warm
      }
      if (a.saturation) {
        const gray = luma(r, g, b)
        r = gray + (r - gray) * sat
        g = gray + (g - gray) * sat
        b = gray + (b - gray) * sat
      }
      d[i] = clamp255(r)
      d[i + 1] = clamp255(g)
      d[i + 2] = clamp255(b)
    }
  }
  if (a.sharpness) sharpen(out, a.sharpness / 100)
  return out
}

// 3x3 unsharp mask on luma; a negative amount softens.
function sharpen(p, amount) {
  const { width: w, height: h, data: d } = p
  const Y = new Float32Array(w * h)
  for (let i = 0, j = 0; j < Y.length; i += 4, j++) Y[j] = luma(d[i], d[i + 1], d[i + 2])
  const k = amount * 1.5
  for (let y = 0; y < h; y++) {
    const ya = y > 0 ? y - 1 : 0
    const yb = y < h - 1 ? y + 1 : h - 1
    for (let x = 0; x < w; x++) {
      const xa = x > 0 ? x - 1 : 0
      const xb = x < w - 1 ? x + 1 : w - 1
      const blur =
        (Y[ya * w + xa] + Y[ya * w + x] + Y[ya * w + xb] + Y[y * w + xa] + Y[y * w + x] + Y[y * w + xb] + Y[yb * w + xa] + Y[yb * w + x] + Y[yb * w + xb]) / 9
      const detail = (Y[y * w + x] - blur) * k
      if (!detail) continue
      const i = (y * w + x) * 4
      d[i] = clamp255(d[i] + detail)
      d[i + 1] = clamp255(d[i + 1] + detail)
      d[i + 2] = clamp255(d[i + 2] + detail)
    }
  }
}

// ---------- selections (masks are Uint8Array, 255 = selected) ----------

// The magic wand: 4-connected pixels whose every channel is within tolerance of the seed.
export function floodSelect(pixels, x, y, tolerance = 32) {
  const { width: w, height: h, data } = pixels
  const mask = new Uint8Array(w * h)
  const sx = Math.floor(x)
  const sy = Math.floor(y)
  if (sx < 0 || sy < 0 || sx >= w || sy >= h) return mask
  const s = (sy * w + sx) * 4
  const seed = [data[s], data[s + 1], data[s + 2], data[s + 3]]
  const near = (j) => {
    const i = j * 4
    return (
      Math.abs(data[i] - seed[0]) <= tolerance &&
      Math.abs(data[i + 1] - seed[1]) <= tolerance &&
      Math.abs(data[i + 2] - seed[2]) <= tolerance &&
      Math.abs(data[i + 3] - seed[3]) <= tolerance
    )
  }
  const stack = [sy * w + sx]
  mask[sy * w + sx] = 255
  while (stack.length) {
    const j = stack.pop()
    const px = j % w
    const py = (j - px) / w
    const around = []
    if (px > 0) around.push(j - 1)
    if (px < w - 1) around.push(j + 1)
    if (py > 0) around.push(j - w)
    if (py < h - 1) around.push(j + w)
    for (const n of around) {
      if (mask[n] || !near(n)) continue
      mask[n] = 255
      stack.push(n)
    }
  }
  return mask
}

export function maskFromRect(width, height, rect) {
  const mask = new Uint8Array(width * height)
  const x0 = clamp(Math.round(rect.x), 0, width)
  const y0 = clamp(Math.round(rect.y), 0, height)
  const x1 = clamp(Math.round(rect.x + rect.w), 0, width)
  const y1 = clamp(Math.round(rect.y + rect.h), 0, height)
  for (let y = y0; y < y1; y++) mask.fill(255, y * width + x0, y * width + x1)
  return mask
}

export function pointInPolygon(x, y, points) {
  let inside = false
  for (let i = 0, j = points.length - 1; i < points.length; j = i++) {
    const [xi, yi] = points[i]
    const [xj, yj] = points[j]
    if (yi > y !== yj > y && x < ((xj - xi) * (y - yi)) / (yj - yi) + xi) inside = !inside
  }
  return inside
}

// Pixel centres inside the polygon (even-odd rule).
export function maskFromLasso(width, height, points) {
  const mask = new Uint8Array(width * height)
  if (!points || points.length < 3) return mask
  const xs = points.map((p) => p[0])
  const ys = points.map((p) => p[1])
  const x0 = clamp(Math.floor(Math.min(...xs)), 0, width)
  const x1 = clamp(Math.ceil(Math.max(...xs)), 0, width)
  const y0 = clamp(Math.floor(Math.min(...ys)), 0, height)
  const y1 = clamp(Math.ceil(Math.max(...ys)), 0, height)
  for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) if (pointInPolygon(x + 0.5, y + 0.5, points)) mask[y * width + x] = 255
  return mask
}

export function maskBounds(mask, width, height) {
  let x0 = width
  let y0 = height
  let x1 = -1
  let y1 = -1
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      if (!mask[y * width + x]) continue
      if (x < x0) x0 = x
      if (x > x1) x1 = x
      if (y < y0) y0 = y
      if (y > y1) y1 = y
    }
  }
  return x1 < 0 ? null : { x: x0, y: y0, w: x1 - x0 + 1, h: y1 - y0 + 1 }
}

// A rect or lasso selection as a mask; a 'mask' selection needs its png decoded (the canvas does it).
export function maskForSelection(selection, width, height) {
  if (selection?.kind === 'rect') return maskFromRect(width, height, selection)
  if (selection?.kind === 'lasso') return maskFromLasso(width, height, selection.points)
  return null
}

// The edges between selected and unselected pixels as one SVG path, runs merged; null if too busy.
export function maskOutline(mask, width, height, limit = 20000) {
  const parts = []
  const at = (x, y) => (x >= 0 && y >= 0 && x < width && y < height ? mask[y * width + x] : 0)
  for (let y = 0; y <= height; y++) {
    let start = -1
    for (let x = 0; x <= width; x++) {
      const edge = x < width && !at(x, y - 1) !== !at(x, y)
      if (edge && start < 0) start = x
      if (!edge && start >= 0) {
        parts.push(`M${start} ${y}H${x}`)
        start = -1
      }
    }
    if (parts.length > limit) return null
  }
  for (let x = 0; x <= width; x++) {
    let start = -1
    for (let y = 0; y <= height; y++) {
      const edge = y < height && !at(x - 1, y) !== !at(x, y)
      if (edge && start < 0) start = y
      if (!edge && start >= 0) {
        parts.push(`M${x} ${start}V${y}`)
        start = -1
      }
    }
    if (parts.length > limit) return null
  }
  return parts.join('')
}

// ---------- painting ----------

// The mask a dab reads: a doc-sized Uint8Array, or { data, width, height, ox, oy } where ox/oy is
// where these pixels sit in the document.
function maskReader(mask, pw, ph) {
  if (!mask) return null
  if (mask instanceof Uint8Array) return (x, y) => mask[y * pw + x]
  const { data, width, height, ox = 0, oy = 0 } = mask
  return (x, y) => {
    const mx = x + ox
    const my = y + oy
    return mx < 0 || my < 0 || mx >= width || my >= height ? 0 : data[my * width + mx]
  }
}

// How much a round brush covers a pixel: full inside size*hardness, fading to nothing at the rim.
export function brushFalloff(dist, radius, hardness) {
  if (dist >= radius) return 0
  const core = radius * clamp(hardness, 0, 1)
  if (dist <= core) return 1
  const t = (dist - core) / (radius - core)
  return 1 - t * t * (3 - 2 * t)
}

function eachDabPixel(p, x, y, size, hardness, mask, visit) {
  const r = Math.max(0.5, size / 2)
  const read = maskReader(mask, p.width, p.height)
  const x0 = clamp(Math.floor(x - r), 0, p.width)
  const x1 = clamp(Math.ceil(x + r), 0, p.width)
  const y0 = clamp(Math.floor(y - r), 0, p.height)
  const y1 = clamp(Math.ceil(y + r), 0, p.height)
  for (let py = y0; py < y1; py++) {
    for (let px = x0; px < x1; px++) {
      let cover = brushFalloff(Math.hypot(px + 0.5 - x, py + 0.5 - y), r, hardness)
      if (!cover) continue
      if (read) cover *= read(px, py) / 255
      if (cover > 0) visit((py * p.width + px) * 4, cover, px, py)
    }
  }
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

function over(d, i, rgb, a) {
  const ab = d[i + 3] / 255
  const ao = a + ab * (1 - a)
  if (ao <= 0) return
  for (let k = 0; k < 3; k++) d[i + k] = (rgb[k] * a + d[i + k] * ab * (1 - a)) / ao
  d[i + 3] = ao * 255
}

// One round dab of colour (rgba 0-255) composited onto the pixels; returns the touched box.
export function dab(pixels, x, y, size, hardness, rgba, opacity = 1, mask = null) {
  const alpha = ((rgba[3] ?? 255) / 255) * opacity
  return eachDabPixel(pixels, x, y, size, hardness, mask, (i, cover) => over(pixels.data, i, rgba, cover * alpha))
}

export function eraseDab(pixels, x, y, size, hardness, opacity = 1, mask = null) {
  const d = pixels.data
  return eachDabPixel(pixels, x, y, size, hardness, mask, (i, cover) => {
    d[i + 3] = d[i + 3] * (1 - cover * opacity)
  })
}

// Copies a soft round patch from `from` to `to`; `source` is a snapshot to read from (default the pixels).
export function cloneDab(pixels, from, to, size, hardness = 0.5, source = pixels, mask = null) {
  const dx = Math.round(from.x - to.x)
  const dy = Math.round(from.y - to.y)
  const s = source.data
  const d = pixels.data
  return eachDabPixel(pixels, to.x, to.y, size, hardness, mask, (i, cover, px, py) => {
    const sx = px + dx
    const sy = py + dy
    if (sx < 0 || sy < 0 || sx >= source.width || sy >= source.height) return
    const j = (sy * source.width + sx) * 4
    const a = (s[j + 3] / 255) * cover
    const keep = 1 - cover
    // mix toward the source pixel, alpha included
    const ao = a + (d[i + 3] / 255) * keep
    if (ao <= 0) {
      d[i + 3] = 0
      return
    }
    for (let k = 0; k < 3; k++) d[i + k] = (s[j + k] * a + d[i + k] * (d[i + 3] / 255) * keep) / ao
    d[i + 3] = ao * 255
  })
}

// A stroke that never builds up past its opacity: `cover` keeps each pixel's strongest touch and the
// touched box is repainted from `base` (the layer as it was when the stroke began).
export function coverDab(cover, width, height, x, y, size, hardness, mask = null) {
  const p = { width, height }
  return eachDabPixel(p, x, y, size, hardness, mask, (i, c) => {
    const j = i >> 2
    if (c > cover[j]) cover[j] = c
  })
}

export function paintCover(target, base, cover, box, { rgba = [0, 0, 0, 255], opacity = 1, erase = false } = {}) {
  const { width } = target
  const t = target.data
  const b = base.data
  const alpha = ((rgba[3] ?? 255) / 255) * opacity
  for (let y = box.y; y < box.y + box.h; y++) {
    for (let x = box.x; x < box.x + box.w; x++) {
      const j = y * width + x
      const i = j * 4
      t[i] = b[i]
      t[i + 1] = b[i + 1]
      t[i + 2] = b[i + 2]
      t[i + 3] = b[i + 3]
      const c = cover[j]
      if (!c) continue
      if (erase) t[i + 3] = b[i + 3] * (1 - c * opacity)
      else over(t, i, rgba, c * alpha)
    }
  }
}

export function hexToRgba(hex) {
  const m = /^#?([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(String(hex || '').trim())
  if (!m) return [0, 0, 0, 255]
  let s = m[1]
  if (s.length === 3) s = [...s].map((c) => c + c).join('')
  return [parseInt(s.slice(0, 2), 16), parseInt(s.slice(2, 4), 16), parseInt(s.slice(4, 6), 16), 255]
}

export function rgbToHex(r, g, b) {
  return `#${[r, g, b].map((v) => Math.round(clamp255(v)).toString(16).padStart(2, '0')).join('')}`
}

// ---------- history ----------

export class History {
  constructor(limit = 40) {
    this.limit = limit
    this.stack = []
    this.index = -1
  }
  reset(state) {
    this.stack = state === undefined ? [] : [state]
    this.index = this.stack.length - 1
  }
  push(state) {
    this.stack = this.stack.slice(0, this.index + 1)
    this.stack.push(state)
    while (this.stack.length > this.limit) this.stack.shift()
    this.index = this.stack.length - 1
  }
  get canUndo() {
    return this.index > 0
  }
  get canRedo() {
    return this.index < this.stack.length - 1
  }
  get current() {
    return this.stack[this.index] ?? null
  }
  undo() {
    if (!this.canUndo) return null
    this.index -= 1
    return this.stack[this.index]
  }
  redo() {
    if (!this.canRedo) return null
    this.index += 1
    return this.stack[this.index]
  }
}

// A name fit for a file: letters, digits, spaces, dashes and underscores.
export function cleanName(name) {
  return String(name || '').replace(/[^\w\- ]+/g, '').replace(/\s+/g, ' ').trim().slice(0, 60) || 'Untitled'
}
