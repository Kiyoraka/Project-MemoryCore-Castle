// PHOTO v2: the view's own pure helpers - the document model v2 (masks, adjustment layers), the
// history with labels, mask geometry and the free-transform math. No DOM, so it can be tested.
import { ADJUST_KEYS, BLENDS, emptyAdjust, newId, normaliseDoc } from './engine.js'

export const HSL_RANGES = ['all', 'reds', 'yellows', 'greens', 'cyans', 'blues', 'magentas']
export const CURVE_CHANNELS = ['rgb', 'r', 'g', 'b']

const clamp = (v, lo, hi) => (v < lo ? lo : v > hi ? hi : v)
const num = (v, fallback = 0) => (Number.isFinite(Number(v)) ? Number(v) : fallback)
const isPng = (v) => typeof v === 'string' && v.startsWith('data:image')
const isHex = (v) => typeof v === 'string' && /^#[0-9a-f]{6}$/i.test(v)

// ---------- v3: the adjustment types PROPERTIES edits with plain controls ----------
// Each field: { key, label, min, max, step, value } (a slider), or type 'check' | 'color' | 'select' (options).
// A key may be a path ('shadows.0', 'reds.c'); a key with {name} in it repeats for every option of the select
// field `name`, and the editor shows the one chosen (its value may be a function of the option). `finish`
// adds what the engine reads but the editor does not show. The params match engine3's adjustments.
const TONES = ['shadows', 'midtones', 'highlights']
const SELECTIVE = ['reds', 'yellows', 'greens', 'cyans', 'blues', 'magentas', 'whites', 'neutrals', 'blacks']
const MIX_OUT = ['red', 'green', 'blue']
export const ADJUSTMENT_SPECS = {
  brightnessContrast: {
    name: 'Brightness/Contrast',
    fields: [
      { key: 'brightness', label: 'Brightness', min: -150, max: 150, step: 1, value: 0 },
      { key: 'contrast', label: 'Contrast', min: -50, max: 100, step: 1, value: 0 },
    ],
  },
  exposure: {
    name: 'Exposure',
    fields: [
      { key: 'exposure', label: 'Exposure', min: -5, max: 5, step: 0.01, value: 0 },
      { key: 'offset', label: 'Offset', min: -0.5, max: 0.5, step: 0.005, value: 0 },
      { key: 'gamma', label: 'Gamma', min: 0.01, max: 9.99, step: 0.01, value: 1 },
    ],
  },
  vibrance: {
    name: 'Vibrance',
    fields: [
      { key: 'vibrance', label: 'Vibrance', min: -100, max: 100, step: 1, value: 0 },
      { key: 'saturation', label: 'Saturation', min: -100, max: 100, step: 1, value: 0 },
    ],
  },
  colorBalance: {
    name: 'Color Balance',
    fields: [
      { key: 'tone', label: 'Tone', type: 'select', options: TONES, value: 'midtones' },
      { key: '{tone}.0', label: 'Cyan - Red', min: -100, max: 100, step: 1, value: 0 },
      { key: '{tone}.1', label: 'Magenta - Green', min: -100, max: 100, step: 1, value: 0 },
      { key: '{tone}.2', label: 'Yellow - Blue', min: -100, max: 100, step: 1, value: 0 },
      { key: 'preserveLuminosity', label: 'Preserve luminosity', type: 'check', value: true },
    ],
  },
  blackWhite: {
    name: 'Black & White',
    fields: [
      { key: 'reds', label: 'Reds', min: -200, max: 300, step: 1, value: 40 },
      { key: 'yellows', label: 'Yellows', min: -200, max: 300, step: 1, value: 60 },
      { key: 'greens', label: 'Greens', min: -200, max: 300, step: 1, value: 40 },
      { key: 'cyans', label: 'Cyans', min: -200, max: 300, step: 1, value: 60 },
      { key: 'blues', label: 'Blues', min: -200, max: 300, step: 1, value: 20 },
      { key: 'magentas', label: 'Magentas', min: -200, max: 300, step: 1, value: 80 },
      { key: 'tintOn', label: 'Tint', type: 'check', value: false },
      { key: 'tintColor', label: 'Tint colour', type: 'color', value: '#e1d3b3' },
    ],
    finish: (p) => ({ ...p, tint: p.tintOn ? p.tintColor : null }),
  },
  photoFilter: {
    name: 'Photo Filter',
    fields: [
      { key: 'color', label: 'Colour', type: 'color', value: '#ec8a00' },
      { key: 'density', label: 'Density', min: 0, max: 100, step: 1, value: 25 },
      { key: 'preserveLuminosity', label: 'Preserve luminosity', type: 'check', value: true },
    ],
  },
  gradientMap: {
    name: 'Gradient Map',
    fields: [
      { key: 'from', label: 'Shadows', type: 'color', value: '#000000' },
      { key: 'to', label: 'Highlights', type: 'color', value: '#ffffff' },
      { key: 'reverse', label: 'Reverse', type: 'check', value: false },
    ],
    finish: (p) => ({ ...p, stops: [{ at: 0, color: p.from }, { at: 1, color: p.to }] }),
  },
  selectiveColor: {
    name: 'Selective Color',
    fields: [
      { key: 'range', label: 'Colours', type: 'select', options: SELECTIVE, value: 'reds' },
      { key: '{range}.c', label: 'Cyan', min: -100, max: 100, step: 1, value: 0 },
      { key: '{range}.m', label: 'Magenta', min: -100, max: 100, step: 1, value: 0 },
      { key: '{range}.y', label: 'Yellow', min: -100, max: 100, step: 1, value: 0 },
      { key: '{range}.k', label: 'Black', min: -100, max: 100, step: 1, value: 0 },
      { key: 'relative', label: 'Relative', type: 'check', value: true },
    ],
  },
  channelMixer: {
    name: 'Channel Mixer',
    fields: [
      { key: 'output', label: 'Output', type: 'select', options: MIX_OUT, value: 'red' },
      { key: '{output}.0', label: 'Red', min: -200, max: 200, step: 1, value: (o) => (o === 'red' ? 100 : 0) },
      { key: '{output}.1', label: 'Green', min: -200, max: 200, step: 1, value: (o) => (o === 'green' ? 100 : 0) },
      { key: '{output}.2', label: 'Blue', min: -200, max: 200, step: 1, value: (o) => (o === 'blue' ? 100 : 0) },
      { key: '{output}.3', label: 'Constant', min: -200, max: 200, step: 1, value: 0 },
      { key: 'monochrome', label: 'Monochrome', type: 'check', value: false },
    ],
  },
  posterize: { name: 'Posterize', fields: [{ key: 'levels', label: 'Levels', min: 2, max: 255, step: 1, value: 4 }] },
  threshold: { name: 'Threshold', fields: [{ key: 'level', label: 'Level', min: 1, max: 255, step: 1, value: 128 }] },
  invert: { name: 'Invert', fields: [] },
  shadowsHighlights: {
    name: 'Shadows/Highlights',
    fields: [
      { key: 'shadows', label: 'Shadows', min: 0, max: 100, step: 1, value: 35 },
      { key: 'highlights', label: 'Highlights', min: 0, max: 100, step: 1, value: 0 },
      { key: 'radius', label: 'Radius', min: 1, max: 200, step: 1, value: 30 },
    ],
  },
  colorLookup: {
    name: 'Color Lookup',
    fields: [
      { key: 'look', label: 'Look', type: 'select', options: ['portra', 'velvia'], value: 'portra' },
      { key: 'amount', label: 'Amount', min: 0, max: 1, step: 0.01, value: 1 },
    ],
  },
}
export const SPEC_TYPES = Object.keys(ADJUSTMENT_SPECS)
export const ADJUSTMENT_TYPES = ['curves', 'levels', 'hsl', 'adjust', ...SPEC_TYPES]
export const ADJUSTMENT_NAMES = {
  curves: 'Curves',
  levels: 'Levels',
  hsl: 'Hue/Saturation',
  adjust: 'Adjust',
  ...Object.fromEntries(SPEC_TYPES.map((t) => [t, ADJUSTMENT_SPECS[t].name])),
}

// The engine knows its looks: the view hands them in (an object of looks, or a list).
export function setLookOptions(looks) {
  const names = (Array.isArray(looks) ? looks : Object.keys(looks || {})).map((l) => (typeof l === 'string' ? l : l?.id || l?.name)).filter(Boolean)
  const field = ADJUSTMENT_SPECS.colorLookup.fields[0]
  if (names.length) {
    field.options = names
    if (!names.includes(field.value)) field.value = names[0]
  }
  return field.options
}

export function getPath(obj, path) {
  let v = obj
  for (const k of String(path).split('.')) {
    if (v == null || typeof v !== 'object') return undefined
    v = v[k]
  }
  return v
}

// A copy of obj with the value at path set; a number step makes an array.
export function setPath(obj, path, value) {
  const keys = String(path).split('.')
  const root = Array.isArray(obj) ? [...obj] : { ...(obj || {}) }
  let at = root
  for (let i = 0; i < keys.length - 1; i++) {
    const k = keys[i]
    const nextIsIndex = /^\d+$/.test(keys[i + 1])
    const cur = at[k]
    at[k] = Array.isArray(cur) ? [...cur] : cur && typeof cur === 'object' ? { ...cur } : nextIsIndex ? [] : {}
    at = at[k]
  }
  at[keys.at(-1)] = value
  return root
}

function cleanField(f, raw, option) {
  const fallback = typeof f.value === 'function' ? f.value(option) : f.value
  if (f.type === 'check') return raw === undefined ? !!fallback : !!raw
  if (f.type === 'color') return isHex(raw) ? raw.toLowerCase() : fallback
  if (f.type === 'select') return f.options.includes(raw) ? raw : fallback
  const v = clamp(num(raw, fallback), f.min, f.max)
  return f.step >= 1 ? Math.round(v) : Math.round(Math.round(v / f.step) * f.step * 1e6) / 1e6
}

// The fields of a type as the editor shows them now: a {name} key resolved with the chosen option.
export function specFields(type, params = {}) {
  const spec = ADJUSTMENT_SPECS[type]
  if (!spec) return []
  return spec.fields.map((f) => {
    const m = /\{(\w+)\}/.exec(f.key)
    if (!m) return f
    const option = params?.[m[1]] ?? spec.fields.find((x) => x.key === m[1])?.value
    return { ...f, key: f.key.replace(m[0], option), value: typeof f.value === 'function' ? f.value(option) : f.value }
  })
}

// The params of a table type, every key present and inside its range (what the engine reads).
export function cleanSpecParams(type, params = {}) {
  const spec = ADJUSTMENT_SPECS[type]
  if (!spec) return {}
  const p = params && typeof params === 'object' ? params : {}
  let out = {}
  for (const f of spec.fields) {
    const m = /\{(\w+)\}/.exec(f.key)
    if (!m) {
      out = setPath(out, f.key, cleanField(f, getPath(p, f.key)))
      continue
    }
    const options = spec.fields.find((x) => x.key === m[1])?.options || []
    for (const o of options) {
      const key = f.key.replace(m[0], o)
      out = setPath(out, key, cleanField(f, getPath(p, key), o))
    }
  }
  return spec.finish ? spec.finish(out) : out
}

// ---------- adjustment layers ----------

export function defaultAdjustment(type) {
  if (type === 'curves') return { type, params: { rgb: [[0, 0], [255, 255]] } }
  if (type === 'levels') return { type, params: { black: 0, gamma: 1, white: 255 } }
  if (type === 'hsl') return { type, params: { ranges: { all: { hue: 0, sat: 0, light: 0 } } } }
  if (ADJUSTMENT_SPECS[type]) return { type, params: cleanSpecParams(type, {}) }
  return { type: 'adjust', params: emptyAdjust() }
}

function cleanPoints(points) {
  const list = (Array.isArray(points) ? points : [])
    .filter((p) => Array.isArray(p) && p.length >= 2)
    .map((p) => [clamp(Math.round(num(p[0])), 0, 255), clamp(Math.round(num(p[1])), 0, 255)])
    .sort((a, b) => a[0] - b[0])
  const out = []
  for (const p of list) if (!out.length || out.at(-1)[0] !== p[0]) out.push(p)
  return out.length >= 2 ? out : null
}

export function normaliseAdjustment(a) {
  const type = ADJUSTMENT_TYPES.includes(a?.type) ? a.type : 'adjust'
  const base = defaultAdjustment(type)
  const p = a?.params && typeof a.params === 'object' ? a.params : {}
  if (type === 'curves') {
    const params = { rgb: cleanPoints(p.rgb) || base.params.rgb }
    for (const ch of ['r', 'g', 'b']) {
      const pts = cleanPoints(p[ch])
      if (pts) params[ch] = pts
    }
    return { type, params }
  }
  if (type === 'levels') {
    const black = clamp(Math.round(num(p.black, 0)), 0, 253)
    const white = clamp(Math.round(num(p.white, 255)), black + 2, 255)
    return { type, params: { black, white, gamma: clamp(num(p.gamma, 1), 0.1, 9.99) } }
  }
  if (type === 'hsl') {
    const ranges = {}
    for (const key of HSL_RANGES) {
      const r = p.ranges?.[key]
      if (!r) continue
      ranges[key] = { hue: clamp(Math.round(num(r.hue)), -180, 180), sat: clamp(Math.round(num(r.sat)), -100, 100), light: clamp(Math.round(num(r.light)), -100, 100) }
    }
    if (!ranges.all) ranges.all = { hue: 0, sat: 0, light: 0 }
    return { type, params: { ranges } }
  }
  if (ADJUSTMENT_SPECS[type]) return { type, params: cleanSpecParams(type, p) }
  const params = emptyAdjust()
  for (const k of ADJUST_KEYS) params[k] = clamp(Math.round(num(p[k])), -100, 100)
  return { type, params }
}

// A document read back from disk (or handed in) with every v2 field present.
export function normaliseDoc2(doc, blends = BLENDS) {
  const base = normaliseDoc(doc)
  const raw = Array.isArray(doc?.layers) ? doc.layers : []
  const layers = base.layers.map((l, i) => {
    const src = raw[i] || {}
    const mask = isPng(src.mask) ? src.mask : null
    const blend = blends.includes(src.blend) ? src.blend : l.blend
    if (src.kind === 'adjustment') {
      const { png, ...rest } = l
      return { ...rest, kind: 'adjustment', blend, adjustment: normaliseAdjustment(src.adjustment), mask }
    }
    return { ...l, blend, mask }
  })
  return { ...base, layers }
}

function uniqueName(doc, base) {
  const names = new Set(doc.layers.map((l) => l.name))
  if (!names.has(base)) return base
  let n = 2
  while (names.has(`${base} ${n}`)) n += 1
  return `${base} ${n}`
}

// The new adjustment layer goes right above `aboveId` (the top when there is none) -> { doc, id }.
export function addAdjustmentLayer(doc, type, aboveId = '') {
  const adjustment = defaultAdjustment(type)
  const names = new Set(doc.layers.map((l) => l.name))
  const word = ADJUSTMENT_NAMES[adjustment.type]
  let n = 1
  while (names.has(`${word} ${n}`)) n += 1
  const layer = { id: newId('adj'), name: `${word} ${n}`, kind: 'adjustment', visible: true, opacity: 1, blend: 'normal', x: 0, y: 0, adjustment, mask: null }
  const at = doc.layers.findIndex((l) => l.id === aboveId)
  const layers = [...doc.layers]
  layers.splice(at < 0 ? layers.length : at + 1, 0, layer)
  return { doc: { ...doc, layers }, id: layer.id }
}

export function setAdjustment(doc, id, params) {
  return {
    ...doc,
    layers: doc.layers.map((l) => (l.id === id && l.kind === 'adjustment' ? { ...l, adjustment: { ...l.adjustment, params } } : l)),
  }
}

export function setMask(doc, id, mask) {
  if (!doc.layers.some((l) => l.id === id)) return doc
  return { ...doc, layers: doc.layers.map((l) => (l.id === id ? { ...l, mask: mask || null } : l)) }
}

// Moves a layer to index `to` (0 = the bottom).
export function moveLayerTo(doc, id, to) {
  const from = doc.layers.findIndex((l) => l.id === id)
  if (from < 0) return doc
  const j = clamp(Math.round(to), 0, doc.layers.length - 1)
  if (j === from) return doc
  const layers = [...doc.layers]
  const [layer] = layers.splice(from, 1)
  layers.splice(j, 0, layer)
  return { ...doc, layers }
}

// A copy right above its original, mask and adjustment included.
export function duplicateLayer2(doc, id) {
  const i = doc.layers.findIndex((l) => l.id === id)
  if (i < 0) return { doc, id: '' }
  const src = doc.layers[i]
  const copy = {
    ...src,
    id: newId(src.kind === 'adjustment' ? 'adj' : 'layer'),
    name: uniqueName(doc, `${src.name} copy`),
    ...(src.text ? { text: { ...src.text } } : {}),
    ...(src.adjustment ? { adjustment: normaliseAdjustment(src.adjustment) } : {}),
  }
  const layers = [...doc.layers]
  layers.splice(i + 1, 0, copy)
  return { doc: { ...doc, layers }, id: copy.id }
}

// What the core keeps in doc.json: image pixels go in their own files, masks stay in the doc.
export function stripForSave(doc) {
  return { ...doc, selection: null, layers: doc.layers.map(({ png, ...rest }) => rest) }
}

// A new image layer that covers the whole document.
export function addFullLayer(doc, png, name, aboveId = '') {
  const layer = { id: newId(), name: uniqueName(doc, name || 'Layer'), kind: 'image', visible: true, opacity: 1, blend: 'normal', x: 0, y: 0, png, mask: null }
  const at = doc.layers.findIndex((l) => l.id === aboveId)
  const layers = [...doc.layers]
  layers.splice(at < 0 ? layers.length : at + 1, 0, layer)
  return { doc: { ...doc, layers }, id: layer.id }
}

// ---------- the history, with a label on every step ----------

export class StepHistory {
  constructor(limit = 40) {
    this.limit = limit
    this.steps = []
    this.index = -1
  }
  reset(state, meta = {}) {
    this.steps = state === undefined ? [] : [{ state, label: meta.label || 'Open', hers: !!meta.hers }]
    this.index = this.steps.length - 1
  }
  push(state, meta = {}) {
    this.steps = this.steps.slice(0, this.index + 1)
    this.steps.push({ state, label: meta.label || 'Edit', hers: !!meta.hers })
    while (this.steps.length > this.limit) this.steps.shift()
    this.index = this.steps.length - 1
  }
  get canUndo() {
    return this.index > 0
  }
  get canRedo() {
    return this.index < this.steps.length - 1
  }
  get current() {
    return this.steps[this.index]?.state ?? null
  }
  undo() {
    if (!this.canUndo) return null
    this.index -= 1
    return this.steps[this.index].state
  }
  redo() {
    if (!this.canRedo) return null
    this.index += 1
    return this.steps[this.index].state
  }
  jump(i) {
    if (!Number.isInteger(i) || i < 0 || i >= this.steps.length) return null
    this.index = i
    return this.steps[i].state
  }
  entries() {
    return this.steps.map(({ label, hers }, i) => ({ label, hers, icon: historyIcon(label), at: i }))
  }
}

const ICON_WORDS = [
  ['open', /open|new document|external/i],
  ['brush', /brush|paint/i],
  ['eraser', /eras/i],
  ['heal', /heal/i],
  ['clone', /clone/i],
  ['curves', /curve/i],
  ['levels', /level/i],
  ['hsl', /hue|saturation/i],
  ['mask', /mask/i],
  ['select', /select|lasso|wand|sky|subject|range|refine/i],
  ['crop', /crop/i],
  ['transform', /rotat|flip|resiz|transform|canvas|size/i],
  ['text', /text/i],
  ['gradient', /gradient/i],
  ['shape', /shape|rect|ellipse|line|polygon/i],
  ['filter', /blur|sharpen|noise|vignette|denoise|fill/i],
  ['adjust', /adjust|exposure|brightness|contrast|warmth|highlight|shadow/i],
  ['layer', /layer|merge|flatten|duplicat|moved|blend|opacity/i],
  ['layer', /group|clip|psd/i],
  ['filter', /style|shadow|glow|stroke|bevel|overlay|liquify|dodge|burn|sponge|smudge|grain|pixelat|emboss|edges|oil|develop|high pass/i],
  ['transform', /artboard|guide|warp|perspective/i],
  ['brush', /path/i],
]

export function historyIcon(label) {
  for (const [icon, re] of ICON_WORDS) if (re.test(String(label || ''))) return icon
  return 'edit'
}

// ---------- masks (Uint8Array, 255 = shown/selected) ----------

export function whiteMask(w, h) {
  return new Uint8Array(w * h).fill(255)
}

// The part of a document-sized mask under a layer of w*h at (x, y), in the layer's own pixels.
export function localMask(mask, W, H, x, y, w, h) {
  const out = new Uint8Array(w * h)
  for (let j = 0; j < h; j++) {
    const dy = j + y
    if (dy < 0 || dy >= H) continue
    for (let i = 0; i < w; i++) {
      const dx = i + x
      if (dx < 0 || dx >= W) continue
      out[j * w + i] = mask[dy * W + dx]
    }
  }
  return out
}

// A document mask moved by (dx, dy); what comes in at the edge is `fill`.
export function shiftMask(mask, W, H, dx, dy, fill = 0) {
  const out = new Uint8Array(W * H).fill(fill)
  for (let y = 0; y < H; y++) {
    const sy = y - dy
    if (sy < 0 || sy >= H) continue
    for (let x = 0; x < W; x++) {
      const sx = x - dx
      if (sx < 0 || sx >= W) continue
      out[y * W + x] = mask[sy * W + sx]
    }
  }
  return out
}

// A document mask on a new canvas of newW*newH, the old one at (dx, dy) in it.
export function placeMask(mask, W, H, newW, newH, dx, dy, fill = 0) {
  const out = new Uint8Array(newW * newH).fill(fill)
  for (let y = 0; y < H; y++) {
    const ny = y + dy
    if (ny < 0 || ny >= newH) continue
    for (let x = 0; x < W; x++) {
      const nx = x + dx
      if (nx < 0 || nx >= newW) continue
      out[ny * newW + nx] = mask[y * W + x]
    }
  }
  return out
}

// Where the old picture sits on a canvas of a new size; anchor like 'center', 'top-left', 'bottom'.
export function canvasOffset(W, H, newW, newH, anchor = 'center') {
  const a = String(anchor || 'center')
  const dx = a.includes('left') ? 0 : a.includes('right') ? newW - W : Math.round((newW - W) / 2)
  const dy = a.includes('top') ? 0 : a.includes('bottom') ? newH - H : Math.round((newH - H) / 2)
  return { dx, dy }
}

// Colour (rgba 0-255) laid over the pixels where the mask allows (all of them without one).
export function fillPixels(pixels, rgba, mask = null) {
  const data = new Uint8ClampedArray(pixels.data)
  const a0 = (rgba[3] ?? 255) / 255
  for (let j = 0, i = 0; i < data.length; j++, i += 4) {
    const a = a0 * (mask ? mask[j] / 255 : 1)
    if (a <= 0) continue
    const ab = data[i + 3] / 255
    const ao = a + ab * (1 - a)
    for (let k = 0; k < 3; k++) data[i + k] = (rgba[k] * a + data[i + k] * ab * (1 - a)) / ao
    data[i + 3] = ao * 255
  }
  return { data, width: pixels.width, height: pixels.height }
}

export function pointsBounds(points) {
  if (!points?.length) return null
  const xs = points.map((p) => (Array.isArray(p) ? p[0] : p.x))
  const ys = points.map((p) => (Array.isArray(p) ? p[1] : p.y))
  const x = Math.min(...xs)
  const y = Math.min(...ys)
  return { x, y, w: Math.max(...xs) - x, h: Math.max(...ys) - y }
}

// ---------- curves and levels ----------

// A new point on the curve, kept in order; a point at the same x replaces it.
export function insertCurvePoint(points, x, y) {
  const px = clamp(Math.round(x), 0, 255)
  const py = clamp(Math.round(y), 0, 255)
  const rest = points.filter((p) => p[0] !== px)
  const next = [...rest, [px, py]].sort((a, b) => a[0] - b[0])
  return { points: next, index: next.findIndex((p) => p[0] === px) }
}

// A point dragged: it stays between its neighbours (the ends stay at the ends).
export function moveCurvePoint(points, i, x, y) {
  const next = points.map((p) => [...p])
  const lo = i > 0 ? next[i - 1][0] + 1 : 0
  const hi = i < next.length - 1 ? next[i + 1][0] - 1 : 255
  next[i] = [clamp(Math.round(x), lo, Math.max(lo, hi)), clamp(Math.round(y), 0, 255)]
  return next
}

export function removeCurvePoint(points, i) {
  if (points.length <= 2) return points
  return points.filter((_, j) => j !== i)
}

// Levels' grey triangle: where gamma puts it between black and white, and back (gamma > 1 brightens).
export function gammaToPos(black, white, gamma) {
  return black + (white - black) * Math.pow(0.5, gamma)
}

export function posToGamma(black, white, pos) {
  const t = clamp((pos - black) / Math.max(1, white - black), 0.01, 0.99)
  return clamp(Math.log(t) / Math.log(0.5), 0.1, 9.99)
}

// ---------- free transform ----------
// t = { w, h, cx, cy, sx, sy, rotate } - the layer's own size, its centre in the document, its scale
// on each side and its turn in radians. Layer pixel (u, v) lands at C + R * S * ((u, v) - (w, h) / 2).

export function startTransform(x, y, w, h) {
  return { w, h, cx: x + w / 2, cy: y + h / 2, sx: 1, sy: 1, rotate: 0 }
}

// The affine as canvas setTransform numbers: X = a*u + c*v + e, Y = b*u + d*v + f.
export function affineFor(t) {
  const cos = Math.cos(t.rotate)
  const sin = Math.sin(t.rotate)
  const a = cos * t.sx
  const b = sin * t.sx
  const c = -sin * t.sy
  const d = cos * t.sy
  return { a, b, c, d, e: t.cx - (a * t.w) / 2 - (c * t.h) / 2, f: t.cy - (b * t.w) / 2 - (d * t.h) / 2 }
}

export function invertAffine(m) {
  const det = m.a * m.d - m.b * m.c || 1e-12
  const a = m.d / det
  const b = -m.b / det
  const c = -m.c / det
  const d = m.a / det
  return { a, b, c, d, e: -(a * m.e + c * m.f), f: -(b * m.e + d * m.f) }
}

export function applyAffine(m, x, y) {
  return { x: m.a * x + m.c * y + m.e, y: m.b * x + m.d * y + m.f }
}

// The four corners in the document: top-left, top-right, bottom-right, bottom-left.
export function transformCorners(t) {
  const m = affineFor(t)
  return [applyAffine(m, 0, 0), applyAffine(m, t.w, 0), applyAffine(m, t.w, t.h), applyAffine(m, 0, t.h)]
}

export function transformBounds(t) {
  const c = transformCorners(t)
  const xs = c.map((p) => p.x)
  const ys = c.map((p) => p.y)
  return { x: Math.min(...xs), y: Math.min(...ys), w: Math.max(...xs) - Math.min(...xs), h: Math.max(...ys) - Math.min(...ys) }
}

// Corner `i` dragged to p, the opposite corner holding still; `uniform` keeps the proportions.
export function scaleFromCorner(t, i, p, uniform = false) {
  const corners = transformCorners(t)
  const anchor = corners[(i + 2) % 4]
  const cos = Math.cos(-t.rotate)
  const sin = Math.sin(-t.rotate)
  const vx0 = p.x - anchor.x
  const vy0 = p.y - anchor.y
  let lx = vx0 * cos - vy0 * sin
  let ly = vx0 * sin + vy0 * cos
  // which way this corner points from the anchor in the layer's own frame
  const signX = i === 1 || i === 2 ? 1 : -1
  const signY = i === 2 || i === 3 ? 1 : -1
  let sx = Math.max(1 / t.w, (lx * signX) / t.w)
  let sy = Math.max(1 / t.h, (ly * signY) / t.h)
  if (uniform) {
    const s = Math.max(sx, sy)
    sx = s
    sy = s
  }
  lx = signX * sx * t.w
  ly = signY * sy * t.h
  const c2 = Math.cos(t.rotate)
  const s2 = Math.sin(t.rotate)
  const cx = anchor.x + (lx * c2 - ly * s2) / 2
  const cy = anchor.y + (lx * s2 + ly * c2) / 2
  return { ...t, sx, sy, cx, cy }
}

// The turn from `from` to `to` around the centre added to the turn it had; snap in degrees.
export function rotateAround(t, rotate0, from, to, snap = 0) {
  let r = rotate0 + Math.atan2(to.y - t.cy, to.x - t.cx) - Math.atan2(from.y - t.cy, from.x - t.cx)
  if (snap) {
    const step = (snap * Math.PI) / 180
    r = Math.round(r / step) * step
  }
  return { ...t, rotate: r }
}

// The numbers PROPERTIES shows during a free transform.
export function transformNumbers(t) {
  const deg = ((((t.rotate * 180) / Math.PI) % 360) + 540) % 360 - 180
  return { x: Math.round(t.cx - (t.w * t.sx) / 2), y: Math.round(t.cy - (t.h * t.sy) / 2), w: Math.round(t.sx * 1000) / 10, h: Math.round(t.sy * 1000) / 10, angle: Math.round(deg * 10) / 10 }
}

export function fromTransformNumbers(t, n) {
  const sx = Math.max(0.001, num(n.w, t.sx * 100) / 100)
  const sy = Math.max(0.001, num(n.h, t.sy * 100) / 100)
  return { ...t, sx, sy, cx: num(n.x) + (t.w * sx) / 2, cy: num(n.y) + (t.h * sy) / 2, rotate: (num(n.angle) * Math.PI) / 180 }
}

// ---------- sizes ----------

export function keepRatio(w0, h0, w, h, changed) {
  if (changed === 'w') return { w, h: Math.max(1, Math.round((w * h0) / w0)) }
  return { w: Math.max(1, Math.round((h * w0) / h0)), h }
}

// What a press at p does to a free-transform box: scale from a corner (within tol), move from
// inside, rotate from outside.
export function transformHit(t, p, tol) {
  const c = transformCorners(t)
  for (let i = 0; i < 4; i++) if (Math.hypot(p.x - c[i].x, p.y - c[i].y) <= tol) return { kind: 'scale', corner: i }
  const q = applyAffine(invertAffine(affineFor(t)), p.x, p.y)
  if (q.x >= 0 && q.y >= 0 && q.x <= t.w && q.y <= t.h) return { kind: 'move' }
  return { kind: 'rotate' }
}

// =====================================================================================================
// PHOTO v3: groups, clipping, layer styles, paragraph text, guides, artboards, the grid, paths - the
// document model, the layer tree the panel shows, and the small pure pieces of the room (rulers, snapping,
// brush presets, text wrapping, the navigator's box, Export As names, actions).
// =====================================================================================================

export const BLEND_MODES_3 = [
  'normal', 'dissolve',
  'darken', 'multiply', 'colorBurn', 'linearBurn', 'darkerColor',
  'lighten', 'screen', 'colorDodge', 'linearDodge', 'lighterColor',
  'overlay', 'softLight', 'hardLight', 'vividLight', 'linearLight', 'pinLight', 'hardMix',
  'difference', 'exclusion', 'subtract', 'divide',
  'hue', 'saturation', 'color', 'luminosity',
]

// 'soft-light' / 'softLight' / 'soft_light' -> 'Soft Light'
export function blendLabel(id) {
  return String(id || '')
    .replace(/([a-z])([A-Z])/g, '$1 $2')
    .replace(/[-_]/g, ' ')
    .replace(/\b\w/g, (c) => c.toUpperCase())
}

// The engine's list as plain ids, whatever shape it comes in.
export function blendIds(list) {
  const ids = (Array.isArray(list) ? list : Object.keys(list || {})).map((b) => (typeof b === 'string' ? b : b?.id || b?.name)).filter(Boolean)
  return ids.length ? ids : BLEND_MODES_3
}

export const STYLE_KEYS = ['dropShadow', 'innerShadow', 'outerGlow', 'innerGlow', 'stroke', 'colorOverlay', 'gradientOverlay', 'bevel']
export const STYLE_NAMES = {
  dropShadow: 'Drop Shadow',
  innerShadow: 'Inner Shadow',
  outerGlow: 'Outer Glow',
  innerGlow: 'Inner Glow',
  stroke: 'Stroke',
  colorOverlay: 'Color Overlay',
  gradientOverlay: 'Gradient Overlay',
  bevel: 'Bevel',
}
const STYLE_BASE = {
  dropShadow: { color: '#000000', opacity: 0.6, blend: 'multiply', angle: 120, distance: 12, spread: 0, size: 24 },
  innerShadow: { color: '#000000', opacity: 0.6, blend: 'multiply', angle: 120, distance: 6, spread: 0, size: 10 },
  outerGlow: { color: '#fff3b0', opacity: 0.75, blend: 'screen', spread: 0, size: 18 },
  innerGlow: { color: '#fff3b0', opacity: 0.75, blend: 'screen', spread: 0, size: 12 },
  stroke: { color: '#ffffff', opacity: 1, blend: 'normal', width: 3, position: 'outside' },
  colorOverlay: { color: '#a98bff', opacity: 1, blend: 'normal' },
  gradientOverlay: { opacity: 1, blend: 'normal', angle: 90, stops: [{ at: 0, color: '#000000' }, { at: 1, color: '#ffffff' }] },
  bevel: { opacity: 0.75, angle: 120, altitude: 30, depth: 100, size: 6, highlight: '#ffffff', shadow: '#000000', direction: 'up' },
}
export const STYLE_POSITIONS = ['outside', 'inside', 'center']

export function defaultStyle(key) {
  return { on: false, ...JSON.parse(JSON.stringify(STYLE_BASE[key] || {})) }
}

function cleanStops3(stops, fallback) {
  const out = (Array.isArray(stops) ? stops : [])
    .slice(0, 8)
    .filter((s) => s && isHex(s.color))
    .map((s) => ({ at: clamp(num(s.at), 0, 1), color: s.color.toLowerCase() }))
    .sort((a, b) => a.at - b.at)
  return out.length >= 2 ? out : fallback
}

// One effect with every field it uses, inside its range.
export function normaliseStyle(key, s, blends = BLEND_MODES_3) {
  const d = defaultStyle(key)
  const src = s && typeof s === 'object' ? s : {}
  const out = { on: !!src.on }
  for (const [k, v] of Object.entries(d)) {
    if (k === 'on') continue
    const raw = src[k]
    if (k === 'color' || k === 'highlight' || k === 'shadow') out[k] = isHex(raw) ? raw.toLowerCase() : v
    else if (k === 'direction') out.direction = raw === 'down' ? 'down' : 'up'
    else if (k === 'altitude') out.altitude = clamp(Math.round(num(raw, v)), 0, 90)
    else if (k === 'blend') out.blend = blends.includes(raw) ? raw : v
    else if (k === 'position') out.position = STYLE_POSITIONS.includes(raw) ? raw : v
    else if (k === 'stops') out.stops = cleanStops3(raw, v)
    else if (k === 'opacity') out.opacity = clamp(num(raw > 1 ? raw / 100 : raw, v), 0, 1)
    else if (k === 'angle') out.angle = Math.round(((num(raw, v) % 360) + 360) % 360)
    else if (k === 'spread') out.spread = clamp(Math.round(num(raw, v)), 0, 100)
    else if (k === 'depth') out.depth = clamp(Math.round(num(raw, v)), 1, 1000)
    else out[k] = clamp(Math.round(num(raw, v)), 0, 250) // distance, size, width
  }
  return out
}

export function normaliseStyles(styles, blends = BLEND_MODES_3) {
  if (!styles || typeof styles !== 'object') return null
  const out = {}
  for (const key of STYLE_KEYS) if (styles[key]) out[key] = normaliseStyle(key, styles[key], blends)
  return Object.keys(out).length ? out : null
}

export const hasStyles = (layer) => !!layer?.styles && STYLE_KEYS.some((k) => layer.styles[k]?.on)
export const styleList = (layer) => STYLE_KEYS.filter((k) => layer?.styles?.[k]).map((k) => ({ key: k, name: STYLE_NAMES[k], on: !!layer.styles[k].on }))

// The paragraph box of a text layer.
export function normaliseTextBox(b) {
  if (!b || typeof b !== 'object') return null
  const w = Math.round(num(b.w))
  if (!(w >= 4)) return null
  return {
    w: clamp(w, 4, 20000),
    h: Math.max(0, Math.round(num(b.h))),
    align: ['left', 'center', 'right', 'justify'].includes(b.align) ? b.align : 'left',
    lineHeight: clamp(num(b.lineHeight, 1.2), 0.5, 5),
    tracking: clamp(Math.round(num(b.tracking)), -200, 1000),
  }
}

// A guide may sit off the canvas, as in Photoshop; wild numbers are bound to a canvas either side.
// A guide's axis in the document is 'v' (a vertical line, at an x) or 'h' (at a y), as her ops write it;
// guideAxis reads either that or 'x' / 'y' and answers 'x' (vertical) or 'y'.
export const guideAxis = (g) => (g?.axis === 'v' || g?.axis === 'x' ? 'x' : g?.axis === 'h' || g?.axis === 'y' ? 'y' : '')
const docAxis = (axis) => (axis === 'x' || axis === 'v' ? 'v' : axis === 'y' || axis === 'h' ? 'h' : '')

function cleanGuides(list, W, H) {
  return (Array.isArray(list) ? list : [])
    .filter((g) => g && guideAxis(g) && Number.isFinite(Number(g.at)))
    .slice(0, 200)
    .map((g) => {
      const axis = guideAxis(g)
      const size = axis === 'x' ? W : H
      return { axis: docAxis(axis), at: Math.round(clamp(Number(g.at), -size, size * 2)) }
    })
}

function cleanArtboards(list) {
  return (Array.isArray(list) ? list : [])
    .filter((a) => a && [a.x, a.y, a.w, a.h].every((v) => Number.isFinite(Number(v))) && Number(a.w) >= 1 && Number(a.h) >= 1)
    .slice(0, 100)
    .map((a, i) => ({
      id: String(a.id || newId('board')),
      name: String(a.name || `Artboard ${i + 1}`).slice(0, 60),
      x: Math.round(Number(a.x)),
      y: Math.round(Number(a.y)),
      w: Math.round(Number(a.w)),
      h: Math.round(Number(a.h)),
    }))
}

function cleanPaths(list) {
  return (Array.isArray(list) ? list : [])
    .filter((p) => p && Array.isArray(p.points) && p.points.length >= 2)
    .slice(0, 100)
    .map((p, i) => ({
      id: String(p.id || newId('path')),
      name: String(p.name || `Path ${i + 1}`).slice(0, 60),
      points: p.points.slice(0, 2000).map((q) => (Array.isArray(q) ? [num(q[0]), num(q[1])] : [num(q?.x), num(q?.y)])),
      closed: !!p.closed,
    }))
}

export function normaliseGrid(g) {
  return { size: clamp(Math.round(num(g?.size, 50)), 2, 2000), show: !!g?.show, snap: g?.snap !== false }
}

// A document read back from disk, handed in, or opened from v1/v2: every v3 field present.
export function normaliseDoc3(doc, blends = BLEND_MODES_3) {
  const raw = Array.isArray(doc?.layers) ? doc.layers : []
  // normaliseDoc2 knows image, text and adjustment layers; groups are put back after it
  const v2 = normaliseDoc2({ ...doc, layers: raw.map((l) => (l?.kind === 'group' ? { ...l, kind: 'image', png: '' } : l)) }, blends)
  const groupIds = new Set(v2.layers.filter((_, i) => raw[i]?.kind === 'group').map((l) => l.id))
  const layers = v2.layers.map((l, i) => {
    const src = raw[i] || {}
    const extra = {}
    if (typeof src.parent === 'string' && src.parent !== l.id && groupIds.has(src.parent)) extra.parent = src.parent
    if (src.clipped) extra.clipped = true
    const styles = normaliseStyles(src.styles, blends)
    if (styles) extra.styles = styles
    if (src.kind === 'group') {
      const { png, ...rest } = l
      // a group may pass its members' blending through
      if (src.blend === 'passThrough') rest.blend = 'passThrough'
      return { ...rest, ...extra, kind: 'group', collapsed: !!src.collapsed }
    }
    if (l.kind === 'text') {
      const box = normaliseTextBox(src.textBox)
      if (box) extra.textBox = box
    }
    return { ...l, ...extra }
  })
  return {
    ...v2,
    layers: fixGroupOrder(layers),
    guides: cleanGuides(doc?.guides, v2.width, v2.height),
    artboards: cleanArtboards(doc?.artboards),
    grid: normaliseGrid(doc?.grid),
    paths: cleanPaths(doc?.paths),
  }
}

// ---------- the layer tree (bottom-first array; a group's members sit right below it, together) ----------

export const isGroup = (l) => l?.kind === 'group'

function withoutParent(l) {
  const { parent, ...rest } = l
  return rest
}

// Every layer whose parent chain reaches `id`.
export function descendantIds(layers, id) {
  const byId = new Map(layers.map((l) => [l.id, l]))
  const out = new Set()
  for (const l of layers) {
    let p = l.parent
    let guard = 0
    while (p && guard++ < 64) {
      if (p === id) {
        out.add(l.id)
        break
      }
      p = byId.get(p)?.parent
    }
  }
  return out
}

// How deep a layer sits (0 at the top level).
export function depthOf(layers, id) {
  const byId = new Map(layers.map((l) => [l.id, l]))
  let d = 0
  let p = byId.get(id)?.parent
  while (p && d < 64) {
    d += 1
    p = byId.get(p)?.parent
  }
  return d
}

// Each group and its members back in one block (an order broken by an old file or a bad move is mended);
// a parent that is not a group is dropped. Order inside each level is kept.
export function fixGroupOrder(layers) {
  const groups = new Set(layers.filter(isGroup).map((l) => l.id))
  const clean = layers.map((l) => (l.parent && !groups.has(l.parent) ? withoutParent(l) : l))
  const byParent = new Map()
  for (const l of clean) {
    const key = l.parent || ''
    if (!byParent.has(key)) byParent.set(key, [])
    byParent.get(key).push(l)
  }
  const out = []
  const seen = new Set()
  const walk = (key, depth) => {
    for (const l of byParent.get(key) || []) {
      if (seen.has(l.id) || depth > 64) continue
      seen.add(l.id)
      if (isGroup(l)) walk(l.id, depth + 1) // members first (below), then the group itself
      out.push(l)
    }
  }
  walk('', 0)
  // a parent loop leaves layers unseen: they come back at the top level
  for (const l of clean) if (!seen.has(l.id)) out.push(withoutParent(l))
  return out
}

// The rows LAYERS shows, top first: { layer, depth, index } - members of a folded group left out.
export function layerRows(layers) {
  const byId = new Map(layers.map((l) => [l.id, l]))
  const rows = []
  for (let i = layers.length - 1; i >= 0; i--) {
    const l = layers[i]
    let p = l.parent
    let hidden = false
    let depth = 0
    while (p && depth < 64) {
      const g = byId.get(p)
      if (!g) break
      if (g.collapsed) hidden = true
      depth += 1
      p = g.parent
    }
    if (!hidden) rows.push({ layer: l, depth, index: i })
  }
  return rows
}

// [lo, hi]: where a layer and (for a group) everything inside it sits in the array.
export function blockRange(layers, id) {
  const hi = layers.findIndex((l) => l.id === id)
  if (hi < 0) return null
  if (!isGroup(layers[hi])) return [hi, hi]
  const inside = descendantIds(layers, id)
  let lo = hi
  while (lo > 0 && inside.has(layers[lo - 1].id)) lo -= 1
  return [lo, hi]
}

function uniqueName3(layers, base) {
  const names = new Set(layers.map((l) => l.name))
  if (!names.has(base)) return base
  let n = 2
  while (names.has(`${base} ${n}`)) n += 1
  return `${base} ${n}`
}

function nextGroupName(layers) {
  const names = new Set(layers.map((l) => l.name))
  let n = 1
  while (names.has(`Group ${n}`)) n += 1
  return `Group ${n}`
}

// Ctrl+G: the chosen layer (with all inside it) into a new group in its place; none -> an empty group on top.
export function groupLayers(doc, id = '') {
  const group = { id: newId('group'), name: nextGroupName(doc.layers), kind: 'group', visible: true, opacity: 1, blend: 'normal', x: 0, y: 0, collapsed: false, mask: null }
  const range = id ? blockRange(doc.layers, id) : null
  const layers = [...doc.layers]
  if (!range) {
    layers.push(group)
    return { doc: { ...doc, layers }, id: group.id }
  }
  const hi = range[1]
  const top = layers[hi]
  if (top.parent) group.parent = top.parent
  layers[hi] = { ...top, parent: group.id }
  layers.splice(hi + 1, 0, group)
  return { doc: { ...doc, layers }, id: group.id }
}

// The group goes; what was inside it stays where it was, one level up.
export function ungroupLayers(doc, groupId) {
  const g = doc.layers.find((l) => l.id === groupId)
  if (!isGroup(g)) return doc
  const layers = doc.layers
    .filter((l) => l.id !== groupId)
    .map((l) => (l.parent !== groupId ? l : g.parent ? { ...l, parent: g.parent } : withoutParent(l)))
  return { ...doc, layers }
}

// A layer (its whole block for a group) dropped on another: 'above' or 'below' it at its level, or 'into' a
// group (on top inside it). A group never goes into itself.
export function placeLayer(doc, id, targetId, mode = 'above') {
  if (!id || id === targetId) return doc
  const range = blockRange(doc.layers, id)
  const target = doc.layers.find((l) => l.id === targetId)
  if (!range || !target || descendantIds(doc.layers, id).has(targetId)) return doc
  const block = doc.layers.slice(range[0], range[1] + 1)
  const rest = [...doc.layers.slice(0, range[0]), ...doc.layers.slice(range[1] + 1)]
  const into = mode === 'into' && isGroup(target)
  const parent = into ? target.id : target.parent || null
  const head = withoutParent(block.at(-1))
  block[block.length - 1] = parent ? { ...head, parent } : head
  let at
  if (into) at = rest.findIndex((l) => l.id === targetId) // just under the group header = its top member
  else if (mode === 'below') at = blockRange(rest, targetId)[0]
  else at = rest.findIndex((l) => l.id === targetId) + 1
  rest.splice(at, 0, ...block)
  return { ...doc, layers: rest }
}

export function toggleClip(doc, id) {
  return { ...doc, layers: doc.layers.map((l) => (l.id !== id ? l : l.clipped ? (({ clipped, ...rest }) => rest)(l) : { ...l, clipped: true })) }
}

export function setGroupFolded(doc, id, collapsed) {
  return { ...doc, layers: doc.layers.map((l) => (l.id === id && isGroup(l) ? { ...l, collapsed: !!collapsed } : l)) }
}

// A layer and, for a group, all inside it. The last non-group layer never goes.
export function deleteLayer3(doc, id) {
  const range = blockRange(doc.layers, id)
  if (!range) return doc
  const layers = [...doc.layers.slice(0, range[0]), ...doc.layers.slice(range[1] + 1)]
  if (!layers.some((l) => !isGroup(l))) return doc
  return { ...doc, layers }
}

// A copy right above the original; a group comes with copies of all inside it.
export function duplicateLayer3(doc, id) {
  const range = blockRange(doc.layers, id)
  if (!range) return { doc, id: '' }
  if (range[0] === range[1]) {
    const made = duplicateLayer2(doc, id)
    if (made.id && doc.layers[range[0]].styles) {
      const i = made.doc.layers.findIndex((l) => l.id === made.id)
      made.doc.layers[i] = { ...made.doc.layers[i], styles: JSON.parse(JSON.stringify(doc.layers[range[0]].styles)) }
    }
    return made
  }
  const remap = new Map()
  const block = doc.layers.slice(range[0], range[1] + 1)
  for (const l of block) remap.set(l.id, newId(isGroup(l) ? 'group' : l.kind === 'adjustment' ? 'adj' : 'layer'))
  const copies = block.map((l) => {
    const c = JSON.parse(JSON.stringify(l))
    c.id = remap.get(l.id)
    if (c.parent && remap.has(c.parent)) c.parent = remap.get(c.parent)
    return c
  })
  const head = copies.at(-1)
  head.name = uniqueName3(doc.layers, `${head.name} copy`)
  const layers = [...doc.layers]
  layers.splice(range[1] + 1, 0, ...copies)
  return { doc: { ...doc, layers }, id: head.id }
}

export function setStyles(doc, id, styles) {
  return { ...doc, layers: doc.layers.map((l) => (l.id !== id ? l : styles ? { ...l, styles } : (({ styles: _s, ...rest }) => rest)(l))) }
}

export function toggleStyle(doc, id, key) {
  const l = doc.layers.find((x) => x.id === id)
  if (!l?.styles?.[key]) return doc
  return setStyles(doc, id, { ...l.styles, [key]: { ...l.styles[key], on: !l.styles[key].on } })
}

// ---------- guides, artboards, paths ----------

export function addGuide(doc, axis, at) {
  const a = docAxis(axis)
  if (!a) return doc
  return { ...doc, guides: [...(doc.guides || []), { axis: a, at: Math.round(at) }] }
}

export function moveGuide(doc, i, at) {
  const guides = [...(doc.guides || [])]
  if (!guides[i]) return doc
  guides[i] = { ...guides[i], at: Math.round(at) }
  return { ...doc, guides }
}

export function removeGuide(doc, i) {
  return { ...doc, guides: (doc.guides || []).filter((_, j) => j !== i) }
}

// Artboards are laid out in a row to the right of the last one (the first at 0, 0).
export function addArtboard(doc, { name, w, h }) {
  const boards = doc.artboards || []
  const last = boards.at(-1)
  const board = {
    id: newId('board'),
    name: String(name || `Artboard ${boards.length + 1}`).slice(0, 60),
    x: last ? last.x + last.w + 80 : 0,
    y: last ? last.y : 0,
    w: Math.max(1, Math.round(w)),
    h: Math.max(1, Math.round(h)),
  }
  return { doc: { ...doc, artboards: [...boards, board] }, board }
}

export function removeArtboard(doc, id) {
  return { ...doc, artboards: (doc.artboards || []).filter((a) => a.id !== id) }
}

export function addPath(doc, points, closed = false, name = '') {
  const paths = doc.paths || []
  const names = new Set(paths.map((p) => p.name))
  let n = paths.length + 1
  while (names.has(`Path ${n}`)) n += 1
  const path = { id: newId('path'), name: name || `Path ${n}`, points: points.map((q) => [Math.round(q[0] * 10) / 10, Math.round(q[1] * 10) / 10]), closed: !!closed }
  return { doc: { ...doc, paths: [...paths, path] }, id: path.id }
}

export function renamePath(doc, id, name) {
  const clean = String(name || '').trim().slice(0, 60)
  if (!clean) return doc
  return { ...doc, paths: (doc.paths || []).map((p) => (p.id === id ? { ...p, name: clean } : p)) }
}

export function removePath(doc, id) {
  return { ...doc, paths: (doc.paths || []).filter((p) => p.id !== id) }
}

// ---------- snapping (used when the engine has no snapTo) ----------

// The lines a moving edge may snap to: guides, the grid (near the values in `around`), canvas and artboard edges.
export function snapLines(doc, { guides = true, grid = true, edges = true } = {}, around = null) {
  const xs = []
  const ys = []
  if (guides) for (const g of doc.guides || []) (guideAxis(g) === 'x' ? xs : ys).push(g.at)
  if (edges) {
    xs.push(0, doc.width / 2, doc.width)
    ys.push(0, doc.height / 2, doc.height)
    for (const a of doc.artboards || []) {
      xs.push(a.x, a.x + a.w / 2, a.x + a.w)
      ys.push(a.y, a.y + a.h / 2, a.y + a.h)
    }
  }
  if (grid && doc.grid?.snap && doc.grid?.show && around) {
    const s = doc.grid.size
    for (const v of around.x || []) xs.push(Math.round(v / s) * s)
    for (const v of around.y || []) ys.push(Math.round(v / s) * s)
  }
  return { xs, ys }
}

// The nearest line within tol of v -> the snapped value (v itself when nothing is near).
export function snapValue(v, lines, tol) {
  let best = v
  let bestD = tol
  for (const l of lines) {
    const d = Math.abs(l - v)
    if (d <= bestD) {
      bestD = d
      best = l
    }
  }
  return best
}

// A box moved by (dx, dy): the nearest of its edges or centre to a line snaps -> the corrected (dx, dy).
export function snapBox(box, dx, dy, lines, tol) {
  const axis = (start, size, d, list) => {
    let best = d
    let bestD = Infinity
    for (const edge of [start + d, start + d + size / 2, start + d + size]) {
      const s = snapValue(edge, list, tol)
      const dd = Math.abs(s - edge)
      if (s !== edge && dd < bestD) {
        bestD = dd
        best = d + (s - edge)
      }
    }
    return best
  }
  return { dx: axis(box.x, box.w, dx, lines.xs), dy: axis(box.y, box.h, dy, lines.ys) }
}

// ---------- rulers ----------

// Ticks for a ruler: a step at least 50 screen px apart (1, 2, 5 x 10^n), across the visible length.
// offset: where document 0 is on the ruler (px); scale: screen px per document px.
export function rulerTicks(scale, offset, length) {
  const s = scale > 0 ? scale : 1
  const raw = 50 / s
  const pow = Math.pow(10, Math.floor(Math.log10(raw)))
  const step = [1, 2, 5, 10].map((m) => m * pow).find((v) => v >= raw - 1e-9)
  const from = Math.floor(-offset / s / step) * step
  const to = (length - offset) / s
  const ticks = []
  for (let v = from; v <= to && ticks.length < 400; v += step) {
    ticks.push({ at: offset + v * s, label: String(Math.round(v)), minor: [0.25, 0.5, 0.75].map((f) => offset + (v + step * f) * s) })
  }
  return { step, ticks }
}

// ---------- brushes ----------

export const BRUSH_PRESETS = [
  { id: 'soft', label: 'Soft round', hardness: 0, flow: 1, build: false, texture: 0 },
  { id: 'hard', label: 'Hard round', hardness: 1, flow: 1, build: false, texture: 0 },
  { id: 'airbrush', label: 'Airbrush', hardness: 0, flow: 0.08, build: true, texture: 0 },
  { id: 'texture', label: 'Texture', hardness: 0.5, flow: 1, build: false, texture: 0.6 },
]

// The size and strength a dab gets from the pen's pressure (0..1). No pressure (0) counts as full.
export function pressured(baseSize, pressure, { size = true, opacity = false } = {}) {
  const p = Number.isFinite(pressure) && pressure > 0 ? clamp(pressure, 0.05, 1) : 1
  return { size: size ? Math.max(1, baseSize * p) : baseSize, strength: opacity ? p : 1 }
}

// A cheap repeatable grain for the texture brush (0..1).
export function grain(x, y) {
  const s = Math.sin(x * 12.9898 + y * 78.233) * 43758.5453
  return s - Math.floor(s)
}

// One dab into a stroke's cover: strength scales it (pen pressure), build adds up (airbrush flow) instead of
// keeping the strongest touch, texture breaks it with grain. mask: Uint8Array of width*height, or
// { data, width, height, ox, oy } (a document mask, these pixels sitting at ox, oy). -> the touched box.
export function presetDab(cover, width, height, x, y, size, hardness, mask = null, { strength = 1, build = false, flow = 1, texture = 0 } = {}) {
  const r = Math.max(0.5, size / 2)
  const x0 = clamp(Math.floor(x - r), 0, width)
  const x1 = clamp(Math.ceil(x + r), 0, width)
  const y0 = clamp(Math.floor(y - r), 0, height)
  const y1 = clamp(Math.ceil(y + r), 0, height)
  const core = r * clamp(hardness, 0, 1)
  for (let py = y0; py < y1; py++) {
    for (let px = x0; px < x1; px++) {
      const dist = Math.hypot(px + 0.5 - x, py + 0.5 - y)
      if (dist >= r) continue
      let c = 1
      if (dist > core) {
        const t = (dist - core) / (r - core)
        c = 1 - t * t * (3 - 2 * t)
      }
      if (mask) {
        let m
        if (mask instanceof Uint8Array) m = mask[py * width + px]
        else {
          const mx = px + (mask.ox || 0)
          const my = py + (mask.oy || 0)
          m = mx < 0 || my < 0 || mx >= mask.width || my >= mask.height ? 0 : mask.data[my * mask.width + mx]
        }
        c *= m / 255
      }
      if (texture) c *= 1 - texture * grain(px, py)
      c *= strength
      if (c <= 0) continue
      const j = py * width + px
      if (build) cover[j] = Math.min(1, cover[j] + c * flow)
      else if (c > cover[j]) cover[j] = c
    }
  }
  return { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
}

// ---------- paragraph text ----------

// The lines of a paragraph box: words wrap at width (measure(text) -> px; tracking adds px per letter gap),
// \n breaks, a word wider than the box breaks by letters.
export function wrapText(value, width, measure, tracking = 0) {
  const widthOf = (s) => measure(s) + tracking * Math.max(0, s.length - 1)
  const lines = []
  for (const para of String(value ?? '').split('\n')) {
    let line = ''
    for (const w of para.split(/(\s+)/).filter((x) => x.length)) {
      const next = line + w
      if (!line.trim() || widthOf(next.trimEnd()) <= width) {
        line = next
      } else {
        lines.push(line.trimEnd())
        line = /^\s+$/.test(w) ? '' : w
      }
      while (widthOf(line.trimEnd()) > width && line.trimEnd().length > 1) {
        line = line.trimEnd()
        let cut = line.length - 1
        while (cut > 1 && widthOf(line.slice(0, cut)) > width) cut -= 1
        lines.push(line.slice(0, cut))
        line = line.slice(cut)
      }
    }
    lines.push(line.trimEnd())
  }
  return lines
}

// Where a line starts in its box for the alignment.
export function lineStart(align, boxW, lineW) {
  if (align === 'center') return (boxW - lineW) / 2
  if (align === 'right') return boxW - lineW
  return 0
}

// ---------- the navigator ----------

// The part of the document the stage shows, in document pixels (inside the picture).
// x, y: where the board's top-left is on the stage; scale: screen px per document px.
export function viewRect({ stageW, stageH, scale, x, y }, W, H) {
  const left = clamp(-x / scale, 0, W) + 0
  const top = clamp(-y / scale, 0, H) + 0
  const right = clamp((stageW - x) / scale, 0, W)
  const bottom = clamp((stageH - y) / scale, 0, H)
  return { x: left, y: top, w: Math.max(0, right - left), h: Math.max(0, bottom - top) }
}

// ---------- Export As ----------

export const EXPORT_SIZES = [16, 32, 48, 64, 128, 180, 192, 256, 512, 1024]

// A file name the core accepts: plain, no folders, no odd letters.
export function exportName(name, suffix, format) {
  const ext = format === 'jpeg' ? 'jpg' : format === 'webp' ? 'webp' : 'png'
  const base =
    String(name || 'image')
      .replace(/[^A-Za-z0-9 _.-]+/g, '')
      .trim()
      .replace(/\s+/g, '-')
      .replace(/^[.-]+/, '')
      .slice(0, 60) || 'image'
  return `${base}${suffix ? `-${suffix}` : ''}.${ext}`
}

// What Export As writes: each chosen artboard (or the whole canvas) at 1x, or at each chosen size (its longer
// side). -> [{ name, rect, w, h }]
export function exportPlan(doc, { boards = [], sizes = [], whole = false, format = 'png', title = 'image' } = {}) {
  const sources = []
  for (const a of doc.artboards || []) if (boards.includes(a.id)) sources.push({ name: a.name, rect: { x: a.x, y: a.y, w: a.w, h: a.h } })
  if (whole || !sources.length) sources.unshift({ name: title, rect: { x: 0, y: 0, w: doc.width, h: doc.height } })
  const out = []
  const used = new Set()
  const unique = (n) => {
    let name = n
    let k = 2
    while (used.has(name)) name = n.replace(/(\.[a-z]+)$/, `-${k++}$1`)
    used.add(name)
    return name
  }
  for (const s of sources) {
    if (!sizes.length) out.push({ name: unique(exportName(s.name, '', format)), rect: s.rect, w: s.rect.w, h: s.rect.h })
    for (const n of sizes) {
      const k = n / Math.max(s.rect.w, s.rect.h)
      out.push({ name: unique(exportName(s.name, `${n}`, format)), rect: s.rect, w: Math.max(1, Math.round(s.rect.w * k)), h: Math.max(1, Math.round(s.rect.h * k)) })
    }
  }
  return out
}

// ---------- actions ----------

export function cleanActions(list) {
  return (Array.isArray(list) ? list : [])
    .filter((a) => a && typeof a.name === 'string' && a.name.trim())
    .slice(0, 200)
    .map((a) => ({ name: a.name.trim().slice(0, 60), steps: (Array.isArray(a.steps) ? a.steps : []).filter((s) => s && typeof s.op === 'string').slice(0, 500) }))
}

export function uniqueActionName(list, base = 'Action') {
  const names = new Set(list.map((a) => a.name))
  if (!names.has(base)) return base
  let n = 2
  while (names.has(`${base} ${n}`)) n += 1
  return `${base} ${n}`
}
