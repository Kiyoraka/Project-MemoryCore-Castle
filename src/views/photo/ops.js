// PHOTO: the companion's ops on the document, each with its own undo.
// v2: the companion's hand works the editor's own native tools (engine2.js); only generate_layer goes to the
// member's image provider (the image queue, on their own key).
// v3: groups, clipping, layer styles, the new adjustments and filters, liquify/warp/perspective, the retouch
// brushes, quick/object select, guides, artboards, Export As, and actions (run, record, batch) - on engine3.js.
// Pure apart from the helpers the view hands in (see HELPERS below) and the engines (injectable for tests).
import { photoAsk } from '../../api/castle.js'
import * as engine2 from './engine2.js'
import * as engine3 from './engine3.js'
import { coverDab, hexToRgba, maskBounds, maskForSelection, paintCover } from './engine.js'

// HELPERS the view gives (all async unless noted):
//   flatten(doc) -> base64          transform(doc, op) -> doc          pixelsOf(doc, layerId) -> pixels (layer-sized)
//   maskOf(doc, layerId) -> Uint8Array(doc w*h)|null                  pngFrom(pixels) -> dataURL
//   maskPngFrom(mask, w, h) -> dataURL                                composite(doc) -> pixels (doc-sized, adjustments in)
//   selectionMask(doc) -> Uint8Array(doc w*h)|null (sync)             play(points, kind) -> Promise (her cursor)
//   selectedId: string | () => string (optional: the layer he has selected)
//   generate(gen) -> { png, costUsd } (ask() fills it from api.photoGenerate)
//   who: the companion's name, for the layers its hand makes (optional; 'Companion' when not given)
// v3:
//   composite3(doc) -> pixels (groups, clipping and styles in; used before composite when given)
//   exportArtboards(doc, sizes|null, { artboards, format }) -> [{ name, image, format }]
//   exportMany(files) -> folder|null (null: he cancelled the folder pick)
//   batchFolder({ action, folder, steps }) -> count          actions: { get() -> [{ name, steps }], set(list) }
//   herOps: [] (optional; ask() fills it with the ops she applied in this ask, for record_action)

export const ADJUST_KEYS = ['exposure', 'brightness', 'contrast', 'highlights', 'shadows', 'warmth', 'saturation', 'sharpness']
export const BLENDS = ['normal', 'multiply', 'screen', 'overlay', 'darken', 'lighten']
export const CHANNELS = ['rgb', 'r', 'g', 'b']
export const RANGES = ['all', 'reds', 'yellows', 'greens', 'cyans', 'blues', 'magentas']
export const FILTERS = ['blur', 'sharpen', 'noise', 'vignette', 'denoise']
export const MASK_FROM = ['selection', 'subject', 'sky', 'invert', 'clear']
export const SHAPES = ['rect', 'ellipse', 'line', 'polygon']
// the sky runs from its top down to the horizon in these colours (warm sunset)
export const SUNSET = ['#7fb6ff', '#ffcf8a', '#ff9a5a']
export const POINTS_CAP = 2000
const TRANSFORMS = ['crop', 'rotate', 'flip', 'resize']
const MAX_SIDE = 12000
const INSET = 0.04

const clamp = (v, lo, hi) => Math.min(hi, Math.max(lo, v))
const isNum = (v) => typeof v === 'number' && Number.isFinite(v)
const cap = (s) => s.charAt(0).toUpperCase() + s.slice(1)
const signed = (n) => (n > 0 ? `+${n}` : `${n}`)
const isHex = (v) => typeof v === 'string' && /^#[0-9a-f]{6}$/i.test(v.trim())
const r2 = (v) => Math.round(v * 100) / 100

function need(ok, message) {
  if (!ok) throw new Error(message)
}

// ---------- the document ----------

const json = (v) => JSON.parse(JSON.stringify(v))

function copyLayer(l) {
  const c = { ...l }
  if (l.text) c.text = { ...l.text }
  if (l.adjustment) c.adjustment = json(l.adjustment)
  if (l.styles) c.styles = json(l.styles)
  if (l.textBox) c.textBox = { ...l.textBox }
  return c
}

// A copy deep enough that nothing we change reaches the input.
export function cloneDoc(doc) {
  const out = {
    ...doc,
    layers: (doc.layers || []).map(copyLayer),
    adjust: { ...(doc.adjust || {}) },
    selection: copySelection(doc.selection),
  }
  if (Array.isArray(doc.guides)) out.guides = doc.guides.map((g) => ({ ...g }))
  if (Array.isArray(doc.artboards)) out.artboards = doc.artboards.map((a) => ({ ...a }))
  if (doc.grid && typeof doc.grid === 'object') out.grid = { ...doc.grid }
  if (Array.isArray(doc.paths)) out.paths = json(doc.paths)
  return out
}

function copySelection(sel) {
  if (!sel) return null
  return sel.points ? { ...sel, points: sel.points.map((p) => (Array.isArray(p) ? [...p] : { ...p })) } : { ...sel }
}

// The doc without any picture data, for her to read; a layer's mask becomes `masked: true`.
export function stripPixels(doc) {
  const out = cloneDoc(doc)
  out.layers = out.layers.map((l) => {
    const { png, mask, ...rest } = l
    return mask ? { ...rest, masked: true } : rest
  })
  if (out.selection && 'png' in out.selection) {
    const { png, ...rest } = out.selection
    out.selection = rest
  }
  return out
}

// A layer id not yet in the doc.
function freshId(doc, prefix) {
  const used = new Set(doc.layers.map((l) => l.id))
  let n = doc.layers.length + 1
  while (used.has(`${prefix}-${n}`)) n++
  return `${prefix}-${n}`
}

// The name the companion's own layers carry: its name, then what the layer is.
const byWho = (helpers, what) => `${(typeof helpers?.who === 'string' && helpers.who.trim()) || 'Companion'} - ${what}`

function newLayer(doc, prefix, name, fields) {
  const layer = { id: freshId(doc, prefix), name, kind: 'image', visible: true, opacity: 1, blend: 'normal', x: 0, y: 0, mask: null, ...fields }
  return layer
}

// A rect kept inside the picture; throws when nothing is left of it.
function rectIn(doc, op) {
  need([op.x, op.y, op.w, op.h].every(isNum), 'the area needs x, y, w and h')
  const x = clamp(Math.round(op.x), 0, doc.width)
  const y = clamp(Math.round(op.y), 0, doc.height)
  const w = clamp(Math.round(op.w), 0, doc.width - x)
  const h = clamp(Math.round(op.h), 0, doc.height - y)
  need(w > 0 && h > 0, 'the area is outside the picture')
  return { x, y, w, h }
}

const hasRect = (op) => [op.x, op.y, op.w, op.h].every(isNum)

// Her points as [x, y] inside the picture, at most `limit`.
function docPoints(doc, points, limit = POINTS_CAP) {
  const out = []
  for (const p of Array.isArray(points) ? points.slice(0, limit) : []) {
    const x = Array.isArray(p) ? p[0] : p?.x
    const y = Array.isArray(p) ? p[1] : p?.y
    if (isNum(x) && isNum(y)) out.push([clamp(Math.round(x), 0, doc.width), clamp(Math.round(y), 0, doc.height)])
  }
  return out
}

function onePoint(doc, p, fallback) {
  return docPoints(doc, [p], 1)[0] || fallback
}

function findByRef(doc, ref) {
  if (ref == null || ref === '') return null
  const byId = doc.layers.find((l) => l.id === ref)
  if (byId) return byId
  const want = String(ref).trim().toLowerCase()
  return doc.layers.find((l) => String(l.name || '').trim().toLowerCase() === want) || null
}

function selectedOf(helpers) {
  const s = typeof helpers.selectedId === 'function' ? helpers.selectedId() : helpers.selectedId
  return typeof s === 'string' ? s : ''
}

// The picture layer she works on: the one she names, else the one he has selected, else the top one shown.
function imageTarget(doc, ref, helpers) {
  if (ref != null && ref !== '') {
    const named = findByRef(doc, ref)
    need(named && named.kind === 'image', `there is no picture layer called ${ref}`)
    return named
  }
  const sel = selectedOf(helpers)
  const chosen = sel && doc.layers.find((l) => l.id === sel && l.kind === 'image')
  if (chosen) return chosen
  const images = doc.layers.filter((l) => l.kind === 'image')
  const top = [...images].reverse().find((l) => l.visible !== false) || images.at(-1)
  need(top, 'there is no picture layer to work on')
  return top
}

// The photo itself: the one she names, else the bottom picture layer.
function photoLayer(doc, ref) {
  if (ref != null && ref !== '') {
    const named = findByRef(doc, ref)
    need(named && named.kind === 'image', `there is no picture layer called ${ref}`)
    return named
  }
  const photo = doc.layers.find((l) => l.kind === 'image')
  need(photo, 'there is no photo to work on')
  return photo
}

function topImageIndex(doc) {
  for (let i = doc.layers.length - 1; i >= 0; i--) if (doc.layers[i].kind === 'image') return i
  return -1
}

// ---------- pixels and masks ----------

function needHelpers(helpers, names) {
  for (const n of names) need(helpers && typeof helpers[n] === 'function', `the editor cannot ${n} yet`)
}

function blank(width, height) {
  return { data: new Uint8ClampedArray(width * height * 4), width, height }
}

function copyPixels(p) {
  return { data: new Uint8ClampedArray(p.data), width: p.width, height: p.height }
}

// engines may change the pixels in place or hand new ones back
const result = (out, pixels) => (out && out.data && isNum(out.width) ? out : pixels)

const anySet = (mask) => !!mask && mask.some((v) => v > 0)

function selMask(doc, helpers) {
  if (!doc.selection) return null
  if (typeof helpers.selectionMask === 'function') return helpers.selectionMask(doc) || null
  return maskForSelection(doc.selection, doc.width, doc.height)
}

// A document-sized mask cut to a layer's own pixels (the layer sits at x, y).
export function docToLayer(mask, docW, docH, layer, pw, ph) {
  const out = new Uint8Array(pw * ph)
  const ox = Math.round(layer.x || 0)
  const oy = Math.round(layer.y || 0)
  for (let y = 0; y < ph; y++) {
    const dy = y + oy
    if (dy < 0 || dy >= docH) continue
    for (let x = 0; x < pw; x++) {
      const dx = x + ox
      if (dx >= 0 && dx < docW) out[y * pw + x] = mask[dy * docW + dx]
    }
  }
  return out
}

// A layer-sized mask placed into the document (outside the layer: 0).
export function layerToDoc(mask, pw, ph, layer, docW, docH) {
  const out = new Uint8Array(docW * docH)
  const ox = Math.round(layer.x || 0)
  const oy = Math.round(layer.y || 0)
  for (let y = 0; y < ph; y++) {
    const dy = y + oy
    if (dy < 0 || dy >= docH) continue
    for (let x = 0; x < pw; x++) {
      const dx = x + ox
      if (dx >= 0 && dx < docW) out[dy * docW + dx] = mask[y * pw + x]
    }
  }
  return out
}

// The selection in a layer's own pixels, or null when nothing is selected.
function layerSel(doc, layer, pixels, helpers) {
  const sel = selMask(doc, helpers)
  return sel ? docToLayer(sel, doc.width, doc.height, layer, pixels.width, pixels.height) : null
}

const centredRect = (w, h, inset = INSET) => ({
  x: Math.round(w * inset),
  y: Math.round(h * inset),
  w: Math.max(1, Math.round(w * (1 - 2 * inset))),
  h: Math.max(1, Math.round(h * (1 - 2 * inset))),
})

const rectOutline = (r) => [[r.x, r.y], [r.x + r.w, r.y], [r.x + r.w, r.y + r.h], [r.x, r.y + r.h], [r.x, r.y]]

function ellipseOutline(r, n = 32) {
  const out = []
  for (let i = 0; i <= n; i++) {
    const a = (i / n) * Math.PI * 2 - Math.PI / 2
    out.push([Math.round(r.x + r.w / 2 + (Math.cos(a) * r.w) / 2), Math.round(r.y + r.h / 2 + (Math.sin(a) * r.h) / 2)])
  }
  return out
}

async function play(helpers, points, kind) {
  if (typeof helpers.play === 'function' && points.length) await helpers.play(points, kind)
}

// A layer's mask is document-sized; null means fully shown.
async function setLayerMask(doc, layer, mask, helpers) {
  layer.mask = mask ? await helpers.maskPngFrom(mask, doc.width, doc.height) : null
}

// ---------- v1: sliders, text, layer fields, rect selection, whole-document ops ----------

function applyAdjust(doc, op) {
  const values = op.values && typeof op.values === 'object' ? op.values : {}
  const next = cloneDoc(doc)
  const before = {}
  const parts = []
  // in her order, known keys only
  for (const key of Object.keys(values).filter((k) => ADJUST_KEYS.includes(k))) {
    const raw = values[key]
    if (!isNum(raw)) continue
    const was = isNum(next.adjust[key]) ? next.adjust[key] : 0
    const amount = Math.round(raw)
    const now = clamp(op.relative ? was + amount : amount, -100, 100)
    before[key] = was
    next.adjust[key] = now
    parts.push(op.relative ? `${cap(key)} ${signed(amount)}` : `${cap(key)} ${now}`)
  }
  need(parts.length > 0, 'no known adjustment was given')
  return { doc: next, undo: { kind: 'adjust', before }, label: parts.join(', ') }
}

function applyText(doc, op) {
  const value = typeof op.value === 'string' ? op.value.trim() : ''
  need(value, 'the text is empty')
  const next = cloneDoc(doc)
  const id = freshId(next, 'text')
  next.layers.push({
    id,
    name: value.length > 24 ? `${value.slice(0, 24)}…` : value,
    kind: 'text',
    visible: true,
    opacity: 1,
    blend: 'normal',
    x: isNum(op.x) ? clamp(Math.round(op.x), 0, doc.width) : Math.round(doc.width * 0.1),
    y: isNum(op.y) ? clamp(Math.round(op.y), 0, doc.height) : Math.round(doc.height * 0.1),
    text: {
      value,
      font: typeof op.font === 'string' && op.font ? op.font : 'Inter',
      size: isNum(op.size) ? clamp(Math.round(op.size), 6, 1000) : 48,
      color: typeof op.color === 'string' && op.color ? op.color : '#ffffff',
      weight: isNum(op.weight) ? op.weight : 600,
    },
  })
  // v3: a paragraph box the words wrap in
  const box = op.box && typeof op.box === 'object' ? cleanTextBox(op.box) : null
  if (box) next.layers.at(-1).textBox = box
  return { doc: next, undo: { kind: 'removeLayer', id }, label: box ? 'Paragraph text added' : 'Text added' }
}

export const TEXT_ALIGNS = ['left', 'center', 'right', 'justify']

function cleanTextBox(box) {
  need(isNum(box.w) && isNum(box.h), 'a text box needs a width and a height')
  return {
    w: Math.round(clamp(box.w, 1, MAX_SIDE)),
    h: Math.round(clamp(box.h, 1, MAX_SIDE)),
    align: TEXT_ALIGNS.includes(box.align) ? box.align : 'left',
    lineHeight: r2(clamp(isNum(box.lineHeight) ? box.lineHeight : 1.2, 0.5, 5)),
    tracking: Math.round(clamp(isNum(box.tracking) ? box.tracking : 0, -200, 1000)),
  }
}

function findLayer(doc, op) {
  if (op.id != null) {
    const byId = doc.layers.find((l) => l.id === op.id)
    if (byId) return byId
  }
  if (typeof op.name === 'string' && op.name.trim()) {
    const want = op.name.trim().toLowerCase()
    const byName = doc.layers.find((l) => String(l.name || '').toLowerCase() === want)
    if (byName) return byName
  }
  if (op.id == null && !op.name) return doc.layers[doc.layers.length - 1]
  return null
}

function applyLayer(doc, op, E3) {
  const next = cloneDoc(doc)
  const layer = findLayer(next, op)
  need(layer, `there is no layer called ${op.name ?? op.id}`)
  const set = op.set && typeof op.set === 'object' ? op.set : {}
  const before = {}
  const parts = []
  if (isNum(set.opacity)) {
    // she may speak in percent
    const v = clamp(set.opacity > 1 ? set.opacity / 100 : set.opacity, 0, 1)
    before.opacity = layer.opacity
    layer.opacity = v
    parts.push(`opacity ${Math.round(v * 100)}%`)
  }
  if (typeof set.blend === 'string' && blendModes(E3).includes(set.blend)) {
    before.blend = layer.blend
    layer.blend = set.blend
    parts.push(`blend ${set.blend}`)
  }
  if (typeof set.visible === 'boolean') {
    before.visible = layer.visible
    layer.visible = set.visible
    parts.push(set.visible ? 'shown' : 'hidden')
  }
  if (typeof set.name === 'string' && set.name.trim()) {
    before.name = layer.name
    layer.name = set.name.trim()
    parts.push(`renamed to ${layer.name}`)
  }
  need(parts.length > 0, 'nothing to change on the layer')
  return { doc: next, undo: { kind: 'layerFields', id: layer.id, before }, label: `Layer: ${parts.join(', ')}` }
}

// Checks a whole-document op and gives it back clean, with its label.
function checkTransform(doc, op) {
  if (op.op === 'crop') {
    const rect = rectIn(doc, op)
    return { clean: { op: 'crop', ...rect }, label: `Cropped to ${rect.w}×${rect.h}` }
  }
  if (op.op === 'rotate') {
    need([1, 2, 3].includes(op.turns), 'a turn is 1, 2 or 3')
    return { clean: { op: 'rotate', turns: op.turns }, label: `Rotated ${op.turns * 90}°` }
  }
  if (op.op === 'flip') {
    need(op.axis === 'h' || op.axis === 'v', 'flip is h or v')
    return { clean: { op: 'flip', axis: op.axis }, label: op.axis === 'h' ? 'Flipped horizontally' : 'Flipped vertically' }
  }
  need(isNum(op.width) && isNum(op.height), 'resize needs a width and a height')
  const width = clamp(Math.round(op.width), 1, MAX_SIDE)
  const height = clamp(Math.round(op.height), 1, MAX_SIDE)
  return { clean: { op: 'resize', width, height }, label: `Resized to ${width}×${height}` }
}

async function applyDocTransform(doc, op, helpers) {
  const { clean, label } = checkTransform(doc, op)
  needHelpers(helpers, ['transform'])
  const before = cloneDoc(doc)
  const next = await helpers.transform(cloneDoc(doc), clean)
  need(next && Array.isArray(next.layers), 'the transform gave nothing back')
  return { doc: next, undo: { kind: 'snapshot', before }, label }
}

// ---------- adjustment layers ----------

function cleanCurve(points) {
  const byX = new Map()
  for (const p of Array.isArray(points) ? points.slice(0, 16) : []) {
    const x = Array.isArray(p) ? p[0] : p?.x
    const y = Array.isArray(p) ? p[1] : p?.y
    if (isNum(x) && isNum(y)) byX.set(clamp(Math.round(x), 0, 255), clamp(Math.round(y), 0, 255))
  }
  const pts = [...byX.entries()].sort((a, b) => a[0] - b[0])
  need(pts.length >= 2, 'a curve needs at least two points')
  return pts
}

async function addAdjustment(doc, type, params, label, helpers) {
  const next = cloneDoc(doc)
  const sel = selMask(doc, helpers)
  let mask = null
  if (sel) {
    needHelpers(helpers, ['maskPngFrom'])
    mask = await helpers.maskPngFrom(sel, doc.width, doc.height)
  }
  const layer = newLayer(next, 'adjust', label, { kind: 'adjustment', mask, adjustment: { type, params } })
  next.layers.push(layer)
  return { doc: next, undo: { kind: 'removeLayer', id: layer.id }, label }
}

function applyCurves(doc, op, helpers) {
  const channel = CHANNELS.includes(op.channel) ? op.channel : 'rgb'
  const points = cleanCurve(op.points)
  const params = channel === 'rgb' ? { rgb: points } : { rgb: [[0, 0], [255, 255]], [channel]: points }
  return addAdjustment(doc, 'curves', params, 'Curves', helpers)
}

function applyLevels(doc, op, helpers) {
  const black = Math.round(clamp(isNum(op.black) ? op.black : 0, 0, 253))
  const white = Math.round(clamp(isNum(op.white) ? op.white : 255, black + 2, 255))
  const gamma = r2(clamp(isNum(op.gamma) ? op.gamma : 1, 0.1, 9.99))
  need(black !== 0 || white !== 255 || gamma !== 1, 'those levels change nothing')
  return addAdjustment(doc, 'levels', { black, gamma, white }, 'Levels', helpers)
}

function applyHsl(doc, op, helpers) {
  const range = RANGES.includes(op.range) ? op.range : 'all'
  const hue = Math.round(clamp(isNum(op.hue) ? op.hue : 0, -180, 180))
  const sat = Math.round(clamp(isNum(op.sat) ? op.sat : 0, -100, 100))
  const light = Math.round(clamp(isNum(op.light) ? op.light : 0, -100, 100))
  need(hue || sat || light, 'that hue/saturation changes nothing')
  return addAdjustment(doc, 'hsl', { ranges: { [range]: { hue, sat, light } } }, 'Hue/Saturation', helpers)
}

// ---------- pixel ops on a picture layer ----------

// Reads the target's pixels, lets `work` change them, writes them back with a layerPng undo.
async function onLayerPixels(doc, layer, helpers, work) {
  needHelpers(helpers, ['pixelsOf', 'pngFrom'])
  const pixels = await helpers.pixelsOf(doc, layer.id)
  need(pixels && pixels.data, 'the layer has no pixels')
  const out = await work(pixels)
  const before = layer.png
  layer.png = await helpers.pngFrom(out)
  return { kind: 'layerPng', id: layer.id, before }
}

const FILTER_LABELS = { blur: 'Blur', sharpen: 'Sharpen', noise: 'Noise', vignette: 'Vignette', denoise: 'Denoise' }

function runFilter(E, name, amount, pixels, mask) {
  const f = amount / 100
  if (name === 'blur') return E.gaussianBlur(pixels, Math.max(0.5, f * 15), mask)
  if (name === 'sharpen') return E.unsharp(pixels, { amount: amount * 2, radius: 1 + f * 2, threshold: 2 }, mask)
  if (name === 'noise') return E.addNoise(pixels, amount * 0.3, 7, mask)
  if (name === 'vignette') return E.vignette(pixels, { amount, size: 50 }, mask)
  return E.median(pixels, Math.max(1, Math.round(f * 4)), mask)
}

async function applyFilter(doc, op, helpers, E) {
  need(FILTERS.includes(op.name), `there is no filter called ${op.name}`)
  const amount = Math.round(clamp(isNum(op.amount) ? op.amount : 50, 0, 100))
  need(amount > 0, 'a filter of 0 changes nothing')
  const next = cloneDoc(doc)
  const layer = imageTarget(next, op.layer, helpers)
  const undo = await onLayerPixels(doc, layer, helpers, (px) => result(runFilter(E, op.name, amount, px, layerSel(doc, layer, px, helpers)), px))
  return { doc: next, undo, label: `${FILTER_LABELS[op.name]} ${amount}` }
}

// Dabs along the points (in the pixels' own space) into a cover, then paints it once: the stroke never builds past its opacity.
function strokeInto(pixels, points, { size, hardness, rgba, opacity, mask }) {
  const { width, height } = pixels
  const cover = new Float32Array(width * height)
  const base = copyPixels(pixels)
  let box = null
  const touch = (x, y) => {
    const b = coverDab(cover, width, height, x, y, size, hardness, mask)
    if (!b || b.w <= 0 || b.h <= 0) return
    if (!box) box = { ...b }
    else {
      const x1 = Math.max(box.x + box.w, b.x + b.w)
      const y1 = Math.max(box.y + box.h, b.y + b.h)
      box.x = Math.min(box.x, b.x)
      box.y = Math.min(box.y, b.y)
      box.w = x1 - box.x
      box.h = y1 - box.y
    }
  }
  const step = Math.max(1, size / 4)
  touch(points[0][0], points[0][1])
  for (let i = 1; i < points.length; i++) {
    const [ax, ay] = points[i - 1]
    const [bx, by] = points[i]
    const n = Math.max(1, Math.ceil(Math.hypot(bx - ax, by - ay) / step))
    for (let k = 1; k <= n; k++) touch(ax + ((bx - ax) * k) / n, ay + ((by - ay) * k) / n)
  }
  if (box) paintCover(pixels, base, cover, box, { rgba, opacity })
  return pixels
}

async function applyBrush(doc, op, helpers) {
  const points = docPoints(doc, op.points)
  need(points.length >= 1, 'a stroke needs points')
  const size = Math.round(clamp(isNum(op.size) ? op.size : 24, 1, 500))
  const opacity = clamp(isNum(op.opacity) ? (op.opacity > 1 ? op.opacity / 100 : op.opacity) : 1, 0, 1)
  const hardness = clamp(isNum(op.hardness) ? op.hardness : 0.5, 0, 1)
  const color = isHex(op.color) ? op.color.trim() : '#ffffff'
  needHelpers(helpers, ['pngFrom'])

  // her hand moves first, then the paint lands
  await play(helpers, points, 'brush')

  const next = cloneDoc(doc)
  const named = op.layer != null && op.layer !== '' ? findByRef(next, op.layer) : null
  const paint = { size, hardness, rgba: hexToRgba(color), opacity }
  if (named && named.kind === 'image') {
    const undo = await onLayerPixels(doc, named, helpers, (px) => {
      const local = points.map(([x, y]) => [x - (named.x || 0), y - (named.y || 0)])
      return strokeInto(px, local, { ...paint, mask: layerSel(doc, named, px, helpers) })
    })
    return { doc: next, undo, label: 'Brush stroke' }
  }
  const px = strokeInto(blank(doc.width, doc.height), points, { ...paint, mask: selMask(doc, helpers) })
  const layer = newLayer(next, 'paint', byWho(helpers, 'paint'), { png: await helpers.pngFrom(px) })
  next.layers.push(layer)
  return { doc: next, undo: { kind: 'removeLayer', id: layer.id }, label: 'Brush stroke' }
}

function cleanStops(stops) {
  const list = Array.isArray(stops) ? stops.slice(0, 8) : []
  const out = []
  list.forEach((s, i) => {
    const color = typeof s === 'string' ? s : s?.color
    if (!isHex(color)) return
    const at = isNum(s?.at) ? clamp(s.at, 0, 1) : list.length > 1 ? i / (list.length - 1) : 0
    const alpha = isNum(s?.alpha) ? clamp(s.alpha, 0, 1) : 1
    out.push({ at, color: color.trim(), alpha })
  })
  return out.sort((a, b) => a.at - b.at)
}

async function newDrawnLayer(doc, prefix, name, helpers, draw) {
  needHelpers(helpers, ['pngFrom'])
  const next = cloneDoc(doc)
  const px = result(await draw(blank(doc.width, doc.height), selMask(doc, helpers)), null)
  need(px, 'nothing was drawn')
  const layer = newLayer(next, prefix, name, { png: await helpers.pngFrom(px) })
  next.layers.push(layer)
  return { next, layer }
}

async function applyGradient(doc, op, helpers, E) {
  const kind = op.kind === 'radial' ? 'radial' : 'linear'
  const from = onePoint(doc, op.from, [Math.round(doc.width / 2), 0])
  const to = onePoint(doc, op.to, [Math.round(doc.width / 2), doc.height])
  const stops = cleanStops(op.stops)
  need(stops.length >= 2, 'a gradient needs two colours')
  await play(helpers, [from, to], 'gradient')
  const spec = { kind, from, to, stops }
  const { next, layer } = await newDrawnLayer(doc, 'gradient', byWho(helpers, 'gradient'), helpers, (px, mask) => result(E.drawGradient(px, spec, mask), px))
  return { doc: next, undo: { kind: 'removeLayer', id: layer.id }, label: 'Gradient' }
}

const SHAPE_LABELS = { rect: 'Rectangle', ellipse: 'Ellipse', line: 'Line', polygon: 'Shape' }

async function applyShape(doc, op, helpers, E) {
  need(SHAPES.includes(op.kind), `there is no shape called ${op.kind}`)
  const fill = isHex(op.fill) ? op.fill.trim() : null
  const stroke = isHex(op.stroke) ? op.stroke.trim() : null
  need(fill || stroke, 'a shape needs a fill or a stroke')
  const width = Math.round(clamp(isNum(op.width) ? op.width : 4, 0, 100))
  const spec = { kind: op.kind, fill, stroke, width }
  let outline
  if (op.kind === 'line' || op.kind === 'polygon') {
    const points = docPoints(doc, op.points, 200)
    need(points.length >= 2, 'that shape needs points')
    spec.points = points
    outline = op.kind === 'polygon' ? [...points, points[0]] : points
  } else {
    Object.assign(spec, rectIn(doc, op))
    outline = op.kind === 'rect' ? rectOutline(spec) : ellipseOutline(spec)
  }
  await play(helpers, outline, 'shape')
  const { next, layer } = await newDrawnLayer(doc, 'shape', byWho(helpers, 'shape'), helpers, (px, mask) => result(E.drawShape(px, spec, mask), px))
  return { doc: next, undo: { kind: 'removeLayer', id: layer.id }, label: SHAPE_LABELS[op.kind] }
}

async function applyLayerTransform(doc, op, helpers, E) {
  const scale = clamp(isNum(op.scale) ? op.scale : 1, 0.05, 20)
  const rotate = clamp(isNum(op.rotate) ? op.rotate : 0, -360, 360)
  const dx = Math.round(clamp(isNum(op.dx) ? op.dx : 0, -doc.width, doc.width))
  const dy = Math.round(clamp(isNum(op.dy) ? op.dy : 0, -doc.height, doc.height))
  need(scale !== 1 || rotate % 360 !== 0 || dx || dy, 'that transform changes nothing')
  needHelpers(helpers, ['pixelsOf', 'pngFrom'])
  const next = cloneDoc(doc)
  const layer = imageTarget(next, op.layer, helpers)
  const pixels = await helpers.pixelsOf(doc, layer.id)
  need(pixels && pixels.data, 'the layer has no pixels')
  const x = layer.x || 0
  const y = layer.y || 0
  // around the layer's centre, in document space
  const cx = x + pixels.width / 2
  const cy = y + pixels.height / 2
  const placed = transformLayerPixels(E, pixels, { x, y, cx, cy, scale, rotate, dx, dy })
  const before = layer.png
  layer.png = await helpers.pngFrom(placed.pixels)
  layer.x = Math.round(placed.x)
  layer.y = Math.round(placed.y)
  const parts = []
  if (scale !== 1) parts.push(`scaled ${Math.round(scale * 100)}%`)
  if (rotate % 360 !== 0) parts.push(`rotated ${Math.round(rotate)}°`)
  if (dx || dy) parts.push('moved')
  return { doc: next, undo: { kind: 'layerPng', id: layer.id, before, x, y }, label: cap(parts.join(', ')) }
}

// engine2's resample -> { pixels, x, y }: the new pixels, grown to hold the turned layer, and where they sit.
function transformLayerPixels(E, pixels, t) {
  const m = E.matrixFor({ scale: t.scale, rotate: t.rotate, dx: t.dx, dy: t.dy, cx: t.cx - t.x, cy: t.cy - t.y })
  const corners = [[0, 0], [pixels.width, 0], [0, pixels.height], [pixels.width, pixels.height]].map(([x, y]) => [m.a * x + m.c * y + m.e, m.b * x + m.d * y + m.f])
  const minX = Math.floor(Math.min(...corners.map((c) => c[0])))
  const minY = Math.floor(Math.min(...corners.map((c) => c[1])))
  const maxX = Math.ceil(Math.max(...corners.map((c) => c[0])))
  const maxY = Math.ceil(Math.max(...corners.map((c) => c[1])))
  const w = clamp(maxX - minX, 1, MAX_SIDE)
  const h = clamp(maxY - minY, 1, MAX_SIDE)
  const out = E.transformPixels(pixels, { ...m, e: m.e - minX, f: m.f - minY }, w, h)
  return { pixels: result(out, pixels), x: t.x + minX, y: t.y + minY }
}

async function applyHeal(doc, op, helpers, E) {
  const points = docPoints(doc, op.points, 500)
  need(points.length >= 1, 'healing needs points')
  const size = Math.round(clamp(isNum(op.size) ? op.size : 24, 1, 300))
  await play(helpers, points, 'brush')
  const next = cloneDoc(doc)
  const layer = imageTarget(next, op.layer, helpers)
  const undo = await onLayerPixels(doc, layer, helpers, (px) => {
    const local = points.map(([x, y]) => [x - (layer.x || 0), y - (layer.y || 0)])
    // a clean patch beside the spot: to the right when there is room, else to the left
    const [fx, fy] = local[0]
    const off = size * 2
    const sx = fx + off + size <= px.width ? fx + off : fx - off
    const source = [clamp(sx, 0, px.width - 1), clamp(fy, 0, px.height - 1)]
    return result(E.heal(px, { source, points: local, size }), px)
  })
  return { doc: next, undo, label: 'Healed' }
}

async function contentAware(doc, holeDoc, op, helpers, E) {
  const next = cloneDoc(doc)
  const layer = imageTarget(next, op.layer, helpers)
  const undo = await onLayerPixels(doc, layer, helpers, (px) => {
    const hole = docToLayer(holeDoc, doc.width, doc.height, layer, px.width, px.height)
    need(anySet(hole), 'that area is not on the picture')
    return result(E.contentAwareFill(px, hole, {}), px)
  })
  return { doc: next, undo, label: 'Erased' }
}

function boxMask(doc, rect) {
  const mask = new Uint8Array(doc.width * doc.height)
  for (let y = rect.y; y < rect.y + rect.h; y++) mask.fill(255, y * doc.width + rect.x, y * doc.width + rect.x + rect.w)
  return mask
}

function applyFill(doc, op, helpers, E) {
  need(op.mode === 'content_aware', 'fill only knows content_aware')
  let hole = selMask(doc, helpers)
  if (!anySet(hole)) {
    need(hasRect(op), 'select what to fill first')
    hole = boxMask(doc, rectIn(doc, op))
  }
  return contentAware(doc, hole, op, helpers, E)
}

function applyErase(doc, op, helpers, E) {
  return contentAware(doc, boxMask(doc, rectIn(doc, op)), op, helpers, E)
}

// ---------- masks and selections ----------

async function compositeOf(doc, helpers) {
  needHelpers(helpers, ['composite'])
  const px = await helpers.composite(doc)
  need(px && px.data, 'the picture could not be read')
  return px
}

async function subjectMask(doc, helpers, E) {
  const comp = await compositeOf(doc, helpers)
  const mask = E.grabCut(comp, centredRect(comp.width, comp.height), {})
  need(anySet(mask), "I couldn't find the subject")
  return mask
}

async function skyMaskOf(doc, helpers, E) {
  const comp = await compositeOf(doc, helpers)
  const mask = E.skyMask(comp, {})
  need(anySet(mask), "I couldn't find any sky")
  return mask
}

const MASK_LABELS = { selection: 'Mask from selection', subject: 'Subject masked', sky: 'Sky masked', invert: 'Mask inverted', clear: 'Mask cleared' }

async function applyMaskOp(doc, op, helpers, E) {
  need(MASK_FROM.includes(op.from), `a mask cannot come from ${op.from}`)
  needHelpers(helpers, ['maskPngFrom'])
  const next = cloneDoc(doc)
  let layer
  if (op.layer != null && op.layer !== '') {
    layer = findByRef(next, op.layer)
    need(layer, `there is no layer called ${op.layer}`)
  } else layer = imageTarget(next, null, helpers)
  const before = layer.mask ?? null
  let mask = null
  if (op.from === 'selection') {
    mask = selMask(doc, helpers)
    need(anySet(mask), 'there is no selection to mask with')
  } else if (op.from === 'subject') mask = await subjectMask(doc, helpers, E)
  else if (op.from === 'sky') mask = await skyMaskOf(doc, helpers, E)
  else if (op.from === 'invert') {
    const current = before && typeof helpers.maskOf === 'function' ? await helpers.maskOf(doc, layer.id) : null
    mask = E.invertMask(current || new Uint8Array(doc.width * doc.height).fill(255))
  } else need(before, 'that layer has no mask')
  await setLayerMask(next, layer, mask, helpers)
  return { doc: next, undo: { kind: 'layerMask', id: layer.id, before }, label: MASK_LABELS[op.from] }
}

async function applySelect(doc, op, helpers, E, E3) {
  const undo = { kind: 'selection', before: copySelection(doc.selection) }
  const next = cloneDoc(doc)
  const asMask = async (mask, label) => {
    needHelpers(helpers, ['maskPngFrom'])
    next.selection = { kind: 'mask', png: await helpers.maskPngFrom(mask, doc.width, doc.height) }
    return { doc: next, undo, label }
  }
  if (op.none === true) {
    next.selection = null
    return { doc: next, undo, label: 'Selection cleared' }
  }
  if (op.quick && typeof op.quick === 'object') return asMask(await selectQuick(doc, op.quick, helpers, E3), 'Quick selected')
  if (op.object && typeof op.object === 'object') return asMask(await selectObject(doc, op.object, helpers, E3), 'Object selected')
  if (op.subject === true) return asMask(await subjectMask(doc, helpers, E), 'Subject selected')
  if (op.sky === true) return asMask(await skyMaskOf(doc, helpers, E), 'Sky selected')
  if (op.invert === true) {
    const current = selMask(doc, helpers)
    need(current, 'there is no selection to invert')
    return asMask(E.invertMask(current), 'Selection inverted')
  }
  if (isHex(op.color)) {
    const fuzz = Math.round(clamp(isNum(op.fuzz) ? op.fuzz : 40, 0, 200))
    const comp = await compositeOf(doc, helpers)
    const mask = E.colorRange(comp, op.color.trim(), fuzz)
    need(anySet(mask), 'no part of the picture has that colour')
    return asMask(mask, 'Colour selected')
  }
  const rect = rectIn(doc, op.rect && typeof op.rect === 'object' ? op.rect : op)
  await play(helpers, rectOutline(rect), 'select')
  next.selection = { kind: 'rect', ...rect }
  return { doc: next, undo, label: 'Area selected' }
}

// ---------- the native replacements of v1's OpenAI edits ----------

async function applyRemoveBackground(doc, op, helpers, E) {
  needHelpers(helpers, ['pixelsOf', 'maskPngFrom'])
  const next = cloneDoc(doc)
  const layer = photoLayer(next, op.layer)
  const pixels = await helpers.pixelsOf(doc, layer.id)
  need(pixels && pixels.data, 'the photo has no pixels')
  const own = E.grabCut(pixels, centredRect(pixels.width, pixels.height), {})
  need(anySet(own), "I couldn't find the subject")
  const before = layer.mask ?? null
  await setLayerMask(next, layer, layerToDoc(own, pixels.width, pixels.height, layer, doc.width, doc.height), helpers)
  return { doc: next, undo: { kind: 'layerMask', id: layer.id, before }, label: 'Background removed' }
}

async function applyReplaceSky(doc, op, helpers, E) {
  needHelpers(helpers, ['pngFrom', 'maskPngFrom'])
  const sky = await skyMaskOf(doc, helpers, E)
  const colors = (Array.isArray(op.colors) ? op.colors : []).filter(isHex).map((c) => c.trim()).slice(0, 6)
  const list = colors.length >= 2 ? colors : SUNSET
  const bounds = maskBounds(sky, doc.width, doc.height) || { x: 0, y: 0, w: doc.width, h: doc.height }
  const spec = {
    kind: 'linear',
    from: [0, bounds.y],
    to: [0, bounds.y + bounds.h],
    stops: list.map((color, i) => ({ at: i / (list.length - 1), color, alpha: 1 })),
  }
  const px = blank(doc.width, doc.height)
  const drawn = result(E.drawGradient(px, spec, null), px)
  const next = cloneDoc(doc)
  const layer = newLayer(next, 'sky', byWho(helpers, 'sky'), {
    png: await helpers.pngFrom(drawn),
    mask: await helpers.maskPngFrom(sky, doc.width, doc.height),
  })
  // just above the top picture layer, so text and shapes stay over it
  next.layers.splice(topImageIndex(next) + 1, 0, layer)
  return { doc: next, undo: { kind: 'removeLayer', id: layer.id }, label: 'Sky replaced' }
}

// ---------- the one op on the image provider ----------

function firstWords(prompt) {
  const words = prompt.split(/\s+/).filter(Boolean).slice(0, 4).join(' ')
  return words.length > 32 ? `${words.slice(0, 32)}…` : words
}

function costText(costUsd) {
  if (!isNum(costUsd) || costUsd <= 0) return ''
  return ` · $${costUsd < 0.01 ? costUsd.toFixed(3) : costUsd.toFixed(2)}`
}

async function applyGenerateLayer(doc, op, helpers) {
  const prompt = typeof op.prompt === 'string' ? op.prompt.trim() : ''
  need(prompt, 'a new layer needs a prompt')
  needHelpers(helpers, ['flatten', 'generate'])
  // the picture as it stands at this op (the doc is passed for asks with several ops)
  const image = await helpers.flatten(doc)
  const out = await helpers.generate({ kind: 'generate_layer', image, prompt })
  need(out && typeof out.png === 'string' && out.png.startsWith('data:image'), 'no picture came back')
  const next = cloneDoc(doc)
  const layer = newLayer(next, 'drawn', byWho(helpers, firstWords(prompt)), { png: out.png })
  next.layers.push(layer)
  return { doc: next, undo: { kind: 'removeLayer', id: layer.id }, label: `Drawn by your image provider${costText(out.costUsd)}` }
}

// ---------- v3: the engine3 constants, read from the engine handed in ----------

export const STYLE_EFFECTS = ['dropShadow', 'innerShadow', 'outerGlow', 'innerGlow', 'stroke', 'colorOverlay', 'gradientOverlay', 'bevel']
export const ADJUSTMENTS3 = [
  'vibrance', 'colorBalance', 'blackWhite', 'photoFilter', 'gradientMap', 'selectiveColor', 'channelMixer',
  'posterize', 'threshold', 'invert', 'exposure', 'shadowsHighlights', 'brightnessContrast', 'colorLookup',
]
export const FILTERS3 = ['motionBlur', 'radialBlur', 'surfaceBlur', 'lensBlur', 'highPass', 'pixelate', 'emboss', 'findEdges', 'oilPaint', 'addGrain', 'develop']
// her op name -> the engine3 brush
export const RETOUCH = { dodge: 'dodge', burn: 'burn', sponge: 'sponge', smudge: 'smudge', blur_brush: 'blurBrush', sharpen_brush: 'sharpenBrush' }
export const LIQUIFY_MODES = ['push', 'bloat', 'pucker', 'reconstruct']
export const TONE_RANGES = ['shadows', 'midtones', 'highlights']
export const EXPORT_FORMATS = ['png', 'jpeg', 'webp']
export const GUIDE_AXES = ['h', 'v']
const STYLE_POSITIONS = ['inside', 'outside', 'center']
const MAX_GUIDES = 200
const MAX_ARTBOARDS = 50
const MAX_EXPORTS = 64
const MAX_STEPS = 50
const MAX_DEPTH = 4
// ops an action never holds: a recording or a batch inside an action would loop
const NOT_IN_ACTIONS = ['record_action', 'batch']

function blendModes(E3) {
  const list = E3 && Array.isArray(E3.BLEND_MODES) ? E3.BLEND_MODES : null
  return list && list.length ? list : BLENDS
}

const ADJ3_LABELS = {
  vibrance: 'Vibrance', colorBalance: 'Color Balance', blackWhite: 'Black & White', photoFilter: 'Photo Filter',
  gradientMap: 'Gradient Map', selectiveColor: 'Selective Color', channelMixer: 'Channel Mixer', posterize: 'Posterize',
  threshold: 'Threshold', invert: 'Invert', exposure: 'Exposure', shadowsHighlights: 'Shadows/Highlights',
  brightnessContrast: 'Brightness/Contrast', colorLookup: 'Color Lookup',
}
const FILTER3_LABELS = {
  motionBlur: 'Motion Blur', radialBlur: 'Radial Blur', surfaceBlur: 'Surface Blur', lensBlur: 'Lens Blur', highPass: 'High Pass',
  pixelate: 'Pixelate', emboss: 'Emboss', findEdges: 'Find Edges', oilPaint: 'Oil Paint', addGrain: 'Grain', develop: 'Develop',
}
const EFFECT_LABELS = {
  dropShadow: 'Drop shadow', innerShadow: 'Inner shadow', outerGlow: 'Outer glow', innerGlow: 'Inner glow',
  stroke: 'Stroke', colorOverlay: 'Colour overlay', gradientOverlay: 'Gradient overlay', bevel: 'Bevel',
}
const RETOUCH_LABELS = { dodge: 'Dodged', burn: 'Burned', sponge: 'Sponged', smudge: 'Smudged', blur_brush: 'Blurred', sharpen_brush: 'Sharpened' }

// engine3 may hand back pixels, { pixels, x, y }, or change them in place
function out3(out, pixels) {
  if (out && out.data && isNum(out.width)) return { pixels: out, x: 0, y: 0 }
  if (out && out.pixels && out.pixels.data) return { pixels: out.pixels, x: isNum(out.x) ? out.x : 0, y: isNum(out.y) ? out.y : 0 }
  return { pixels, x: 0, y: 0 }
}

function engineFn(E3, name) {
  const fn = E3 && E3[name]
  need(typeof fn === 'function', `the editor has no ${name} yet`)
  return fn
}

// Free-form engine params, kept small and plain: numbers clamped, short strings, shallow arrays and objects.
function cleanParams(v, depth = 0) {
  if (isNum(v)) return r2(clamp(v, -10000, 10000))
  if (typeof v === 'boolean') return v
  if (typeof v === 'string') return v.trim().slice(0, 40)
  if (depth >= 3) return undefined
  if (Array.isArray(v)) return v.slice(0, 16).map((x) => cleanParams(x, depth + 1)).filter((x) => x !== undefined)
  if (v && typeof v === 'object') {
    const out = {}
    for (const [k, x] of Object.entries(v).slice(0, 24)) {
      if (!/^[A-Za-z0-9_]{1,24}$/.test(k)) continue
      const c = cleanParams(x, depth + 1)
      if (c !== undefined) out[k] = c
    }
    return out
  }
  return undefined
}

// depthMask (lens blur) is a picture, never something she writes
const NEVER_PARAMS = ['depthMask']

function paramsOf(op, table) {
  const raw = op.params && typeof op.params === 'object' && !Array.isArray(op.params) ? op.params : {}
  const known = table && table.params && typeof table.params === 'object' ? Object.keys(table.params) : null
  const picked = Object.fromEntries(Object.entries(raw).filter(([k]) => !NEVER_PARAMS.includes(k) && (!known || known.includes(k))))
  return cleanParams(picked) || {}
}

const tableOf = (E3, name, key) => (E3 && E3[name] && E3[name][key]) || null

// A layer of any kind, by name or id; throws with her words when it is not there.
function anyLayer(doc, ref, helpers, what = 'layer') {
  if (ref != null && ref !== '') {
    const l = findByRef(doc, ref)
    need(l, `there is no ${what} called ${ref}`)
    return l
  }
  const sel = selectedOf(helpers)
  const chosen = sel && doc.layers.find((l) => l.id === sel)
  if (chosen) return chosen
  need(doc.layers.length, 'there is no layer')
  return doc.layers.at(-1)
}

const localPoints = (points, layer) => points.map(([x, y]) => [x - (layer.x || 0), y - (layer.y || 0)])

// ---------- v3: groups ----------

function groupIdsOf(doc, groupId) {
  // every layer inside a group, however deep
  const out = new Set()
  let grew = true
  while (grew) {
    grew = false
    for (const l of doc.layers) {
      if (!out.has(l.id) && (l.parent === groupId || out.has(l.parent))) {
        out.add(l.id)
        grew = true
      }
    }
  }
  return out
}

function freshGroupName(doc) {
  let n = doc.layers.filter((l) => l.kind === 'group').length + 1
  const names = new Set(doc.layers.map((l) => String(l.name || '').toLowerCase()))
  while (names.has(`group ${n}`)) n++
  return `Group ${n}`
}

function applyGroup(doc, op) {
  const refs = Array.isArray(op.layers) ? op.layers.slice(0, 100) : []
  need(refs.length >= 1, 'a group needs layers')
  const next = cloneDoc(doc)
  const picked = new Set()
  for (const ref of refs) {
    const l = findByRef(next, ref)
    need(l, `there is no layer called ${ref}`)
    picked.add(l.id)
  }
  // the picked layers that are not already inside another picked group
  const tops = next.layers.filter((l) => picked.has(l.id) && !hasPickedAncestor(next, l, picked))
  const moving = new Set()
  for (const l of tops) {
    moving.add(l.id)
    if (l.kind === 'group') for (const id of groupIdsOf(next, l.id)) moving.add(id)
  }
  const parents = [...new Set(tops.map((l) => l.parent ?? null))]
  const parent = parents.length === 1 ? parents[0] : null
  const oldOrder = next.layers.map((l) => l.id)
  const oldParents = Object.fromEntries(tops.map((l) => [l.id, l.parent ?? null]))
  // the group goes where the topmost of them was; its layers sit together just below it
  const topIndex = Math.max(...tops.map((t) => next.layers.indexOf(t)))
  const keepBelow = next.layers.slice(0, topIndex + 1).filter((l) => !moving.has(l.id))
  const keepAbove = next.layers.slice(topIndex + 1).filter((l) => !moving.has(l.id))
  const inside = next.layers.filter((l) => moving.has(l.id))
  const name = typeof op.name === 'string' && op.name.trim() ? op.name.trim().slice(0, 60) : freshGroupName(next)
  const group = { id: freshId(next, 'group'), name, kind: 'group', visible: true, opacity: 1, blend: 'normal', x: 0, y: 0, mask: null, collapsed: false }
  if (parent) group.parent = parent
  for (const l of tops) l.parent = group.id
  next.layers = [...keepBelow, ...inside, group, ...keepAbove]
  return { doc: next, undo: { kind: 'ungroup', id: group.id, parents: oldParents, order: oldOrder }, label: `Grouped ${tops.length} layer${tops.length === 1 ? '' : 's'}` }
}

function hasPickedAncestor(doc, layer, picked) {
  let p = layer.parent
  const seen = new Set()
  while (p && !seen.has(p)) {
    if (picked.has(p)) return true
    seen.add(p)
    p = doc.layers.find((l) => l.id === p)?.parent
  }
  return false
}

function applyUngroup(doc, op) {
  const next = cloneDoc(doc)
  const group = findByRef(next, op.group)
  need(group && group.kind === 'group', `there is no group called ${op.group}`)
  const index = next.layers.indexOf(group)
  const children = next.layers.filter((l) => l.parent === group.id)
  for (const c of children) {
    if (group.parent) c.parent = group.parent
    else delete c.parent
  }
  next.layers = next.layers.filter((l) => l.id !== group.id)
  const below = index > 0 ? doc.layers[index - 1].id : null
  return { doc: next, undo: { kind: 'regroup', layer: copyLayer(group), below, children: children.map((c) => c.id) }, label: `Ungrouped ${group.name}` }
}

// Puts the moved layers back in their old order among whatever is there now.
function restoreOrder(layers, order, moved) {
  const byId = new Map(layers.map((l) => [l.id, l]))
  const out = layers.filter((l) => !moved.has(l.id))
  let last = null
  for (const id of order) {
    if (!byId.has(id)) continue
    if (moved.has(id)) {
      const at = last == null ? 0 : out.findIndex((l) => l.id === last) + 1
      out.splice(at, 0, byId.get(id))
      last = id
    } else if (out.some((l) => l.id === id)) last = id
  }
  return out
}

// ---------- v3: clip, blend, styles ----------

function applyClip(doc, op, helpers) {
  const next = cloneDoc(doc)
  const layer = anyLayer(next, op.layer, helpers)
  const on = op.on !== false
  if (on) need(next.layers.indexOf(layer) > 0, 'there is nothing below that layer to clip to')
  const was = layer.clipped === true
  need(was !== on, on ? 'that layer is already clipped' : 'that layer is not clipped')
  layer.clipped = on
  return { doc: next, undo: { kind: 'layerFields', id: layer.id, before: { clipped: was } }, label: on ? `Clipped ${layer.name}` : `Unclipped ${layer.name}` }
}

function applyBlend(doc, op, helpers, E3) {
  const next = cloneDoc(doc)
  const layer = anyLayer(next, op.layer, helpers)
  // a group may also pass its layers through to what is below (compose3's 'passThrough')
  const modes = layer.kind === 'group' ? [...blendModes(E3), 'passThrough'] : blendModes(E3)
  need(typeof op.mode === 'string' && modes.includes(op.mode), `there is no blend mode called ${op.mode}`)
  const before = { blend: layer.blend ?? 'normal' }
  layer.blend = op.mode
  return { doc: next, undo: { kind: 'layerFields', id: layer.id, before }, label: `Blend ${op.mode}` }
}

function cleanEffect(set, E3) {
  const out = {}
  const n = (key, lo, hi, round = true) => {
    if (isNum(set[key])) out[key] = round ? Math.round(clamp(set[key], lo, hi)) : r2(clamp(set[key], lo, hi))
  }
  if (typeof set.on === 'boolean') out.on = set.on
  if (isHex(set.color)) out.color = set.color.trim().toLowerCase()
  if (isNum(set.opacity)) out.opacity = r2(clamp(set.opacity > 1 ? set.opacity / 100 : set.opacity, 0, 1))
  if (typeof set.blend === 'string' && blendModes(E3).includes(set.blend)) out.blend = set.blend
  n('angle', -360, 360)
  n('distance', 0, 1000)
  n('spread', 0, 100)
  n('size', 0, 250)
  n('width', 1, 250)
  n('depth', 1, 1000)
  n('altitude', 0, 90)
  if (STYLE_POSITIONS.includes(set.position)) out.position = set.position
  if (set.direction === 'up' || set.direction === 'down') out.direction = set.direction
  if (isHex(set.highlight)) out.highlight = set.highlight.trim().toLowerCase()
  if (isHex(set.shadow)) out.shadow = set.shadow.trim().toLowerCase()
  if (isNum(set.shadowOpacity)) out.shadowOpacity = r2(clamp(set.shadowOpacity > 1 ? set.shadowOpacity / 100 : set.shadowOpacity, 0, 1))
  if (set.kind === 'linear' || set.kind === 'radial') out.kind = set.kind
  if (typeof set.reverse === 'boolean') out.reverse = set.reverse
  if (set.stops != null) {
    const stops = cleanStops(set.stops)
    need(stops.length >= 2, 'a gradient needs two colours')
    out.stops = stops
  }
  return out
}

function applyStyle(doc, op, helpers, E3) {
  need(STYLE_EFFECTS.includes(op.effect), `there is no layer style called ${op.effect}`)
  const set = op.set && typeof op.set === 'object' ? op.set : {}
  const next = cloneDoc(doc)
  const layer = anyLayer(next, op.layer, helpers)
  const clean = cleanEffect(set, E3)
  const old = layer.styles && layer.styles[op.effect] ? layer.styles[op.effect] : null
  layer.styles = { ...(layer.styles || {}), [op.effect]: { ...(old || {}), ...clean, on: clean.on ?? true } }
  const on = layer.styles[op.effect].on
  return {
    doc: next,
    undo: { kind: 'layerStyle', id: layer.id, effect: op.effect, before: old ? json(old) : null },
    label: on ? EFFECT_LABELS[op.effect] : `${EFFECT_LABELS[op.effect]} off`,
  }
}

// ---------- v3: adjustments and filters ----------

function looksOf(E3) {
  const L = E3 && E3.LOOKS
  if (Array.isArray(L)) return L.map((x) => (typeof x === 'string' ? x : x?.name)).filter(Boolean)
  return L && typeof L === 'object' ? Object.keys(L) : []
}

function applyAdjustment3(doc, op, helpers, E3) {
  const type = op.type
  const raw = op.params && typeof op.params === 'object' ? op.params : {}
  if (type === 'curves') return applyCurves(doc, raw, helpers)
  if (type === 'levels') return applyLevels(doc, raw, helpers)
  if (type === 'hsl') return applyHsl(doc, raw, helpers)
  need(ADJUSTMENTS3.includes(type), `there is no adjustment called ${type}`)
  const table = tableOf(E3, 'ADJUSTMENTS', type)
  const params = paramsOf(op, table)
  if (type === 'posterize') params.levels = Math.round(clamp(isNum(params.levels) ? params.levels : 4, 2, 255))
  if (type === 'threshold') params.level = Math.round(clamp(isNum(params.level) ? params.level : 128, 0, 255))
  if (type === 'gradientMap') {
    const stops = cleanStops(raw.stops)
    need(stops.length >= 2, 'a gradient map needs two colours')
    params.stops = stops
  }
  if (type === 'colorLookup') {
    const looks = looksOf(E3)
    need(typeof params.look === 'string' && (!looks.length || looks.includes(params.look)), `there is no look called ${params.look}`)
  }
  return addAdjustment(doc, type, params, table?.label || ADJ3_LABELS[type], helpers)
}

async function applyFilter3(doc, op, helpers, E3) {
  const fn = engineFn(E3, op.name)
  const table = tableOf(E3, 'FILTERS', op.name)
  const params = paramsOf(op, table)
  if (isNum(op.amount) && !('amount' in params) && (!table || 'amount' in (table.params || {}))) params.amount = Math.round(clamp(op.amount, 0, 100))
  const next = cloneDoc(doc)
  const layer = imageTarget(next, op.layer, helpers)
  const undo = await onLayerPixels(doc, layer, helpers, (px) => out3(fn(px, params, layerSel(doc, layer, px, helpers)), px).pixels)
  return { doc: next, undo, label: table?.label || FILTER3_LABELS[op.name] }
}

// ---------- v3: liquify, warp, perspective ----------

function cleanStroke(doc, s) {
  const points = docPoints(doc, s?.points, 500)
  need(points.length >= 1, 'a stroke needs points')
  return {
    mode: s.mode === 'forward' ? 'push' : LIQUIFY_MODES.includes(s.mode) ? s.mode : 'push',
    points,
    size: Math.round(clamp(isNum(s.size) ? s.size : 60, 2, 1000)),
    pressure: r2(clamp(isNum(s.pressure) ? (s.pressure > 1 ? s.pressure / 100 : s.pressure) : 0.5, 0.01, 1)),
  }
}

async function applyLiquify(doc, op, helpers, E3) {
  const list = Array.isArray(op.strokes) ? op.strokes.slice(0, 50) : []
  need(list.length >= 1, 'liquify needs strokes')
  const strokes = list.map((s) => cleanStroke(doc, s))
  const makeMesh = engineFn(E3, 'liquifyMesh')
  const stroke = engineFn(E3, 'liquifyStroke')
  const applyMesh = engineFn(E3, 'applyMesh')
  for (const s of strokes) await play(helpers, s.points, 'liquify')
  const next = cloneDoc(doc)
  const layer = imageTarget(next, op.layer, helpers)
  const undo = await onLayerPixels(doc, layer, helpers, (px) => {
    let mesh = makeMesh(px.width, px.height)
    for (const s of strokes) mesh = stroke(mesh, { ...s, points: localPoints(s.points, layer) }) || mesh
    return out3(applyMesh(px, mesh), px).pixels
  })
  return { doc: next, undo, label: `Liquified (${strokes.length} stroke${strokes.length === 1 ? '' : 's'})` }
}

// A geometric op that may grow the layer: new pixels and where they sit, with a layerPng + x/y undo.
async function placeLayerPixels(doc, op, helpers, work, label) {
  needHelpers(helpers, ['pixelsOf', 'pngFrom'])
  const next = cloneDoc(doc)
  const layer = imageTarget(next, op.layer, helpers)
  const pixels = await helpers.pixelsOf(doc, layer.id)
  need(pixels && pixels.data, 'the layer has no pixels')
  const x = layer.x || 0
  const y = layer.y || 0
  const placed = out3(await work(pixels, layer), pixels)
  const before = layer.png
  layer.png = await helpers.pngFrom(placed.pixels)
  layer.x = Math.round(x + placed.x)
  layer.y = Math.round(y + placed.y)
  return { doc: next, undo: { kind: 'layerPng', id: layer.id, before, x, y }, label }
}

function cleanGrid(doc, grid) {
  // a 4x4 mesh as 16 [x, y] points (rows top to bottom), in document space
  const flat = Array.isArray(grid) && Array.isArray(grid[0]) && Array.isArray(grid[0][0]) ? grid.flat() : grid
  const points = []
  for (const p of Array.isArray(flat) ? flat.slice(0, 16) : []) {
    const x = Array.isArray(p) ? p[0] : p?.x
    const y = Array.isArray(p) ? p[1] : p?.y
    if (isNum(x) && isNum(y)) points.push([Math.round(clamp(x, -doc.width, doc.width * 2)), Math.round(clamp(y, -doc.height, doc.height * 2))])
  }
  need(points.length === 16, 'a warp grid needs 16 points (4 by 4)')
  return points
}

// engine3 renders into a width x height canvas at the layer's own (0, 0): size it to the box of the
// new shape and move the layer there.
function intoBox(fn, px, points) {
  const xs = points.map((p) => p[0])
  const ys = points.map((p) => p[1])
  const x = Math.floor(Math.min(...xs))
  const y = Math.floor(Math.min(...ys))
  const width = clamp(Math.ceil(Math.max(...xs)) - x, 1, MAX_SIDE)
  const height = clamp(Math.ceil(Math.max(...ys)) - y, 1, MAX_SIDE)
  const out = fn(px, points.map(([a, b]) => [a - x, b - y]), { width, height })
  return { pixels: out3(out, px).pixels, x, y }
}

async function applyWarp(doc, op, helpers, E3) {
  const grid = cleanGrid(doc, op.grid)
  const fn = engineFn(E3, 'meshWarp')
  await play(helpers, grid, 'warp')
  return placeLayerPixels(doc, op, helpers, (px, layer) => intoBox(fn, px, localPoints(grid, layer)), 'Warped')
}

async function applyPerspective(doc, op, helpers, E3) {
  const corners = []
  for (const p of Array.isArray(op.corners) ? op.corners.slice(0, 4) : []) {
    const x = Array.isArray(p) ? p[0] : p?.x
    const y = Array.isArray(p) ? p[1] : p?.y
    if (isNum(x) && isNum(y)) corners.push([Math.round(clamp(x, -doc.width, doc.width * 2)), Math.round(clamp(y, -doc.height, doc.height * 2))])
  }
  need(corners.length === 4, 'perspective needs four corners')
  const fn = engineFn(E3, 'perspective')
  await play(helpers, [...corners, corners[0]], 'perspective')
  return placeLayerPixels(doc, op, helpers, (px, layer) => intoBox(fn, px, localPoints(corners, layer)), 'Perspective')
}

// ---------- v3: the retouch brushes, spot heal and patch ----------

async function applyRetouch(doc, op, helpers, E3) {
  const fn = engineFn(E3, RETOUCH[op.op])
  const points = docPoints(doc, op.points, 1000)
  need(points.length >= 1, 'a stroke needs points')
  const size = Math.round(clamp(isNum(op.size) ? op.size : 40, 1, 500))
  const strength = r2(clamp(isNum(op.strength) ? (op.strength > 1 ? op.strength / 100 : op.strength) : 0.5, 0.01, 1))
  const spec = { size, strength }
  if (op.op === 'dodge' || op.op === 'burn') spec.range = TONE_RANGES.includes(op.range) ? op.range : 'midtones'
  if (op.op === 'sponge') spec.mode = op.mode === 'desaturate' ? 'desaturate' : 'saturate'
  await play(helpers, points, 'brush')
  const next = cloneDoc(doc)
  const layer = imageTarget(next, op.layer, helpers)
  const undo = await onLayerPixels(doc, layer, helpers, (px) => out3(fn(px, { ...spec, points: localPoints(points, layer) }, layerSel(doc, layer, px, helpers)), px).pixels)
  return { doc: next, undo, label: RETOUCH_LABELS[op.op] }
}

async function applySpotHeal(doc, op, helpers, E3) {
  const fn = engineFn(E3, 'spotHeal')
  const points = docPoints(doc, op.points, 500)
  need(points.length >= 1, 'spot healing needs points')
  const size = Math.round(clamp(isNum(op.size) ? op.size : 24, 1, 300))
  await play(helpers, points, 'brush')
  const next = cloneDoc(doc)
  const layer = imageTarget(next, op.layer, helpers)
  const undo = await onLayerPixels(doc, layer, helpers, (px) => out3(fn(px, { points: localPoints(points, layer), size }, layerSel(doc, layer, px, helpers)), px).pixels)
  return { doc: next, undo, label: 'Spot healed' }
}

async function applyPatch(doc, op, helpers, E3) {
  const fn = engineFn(E3, 'patch')
  need(isNum(op.dx) && isNum(op.dy) && (op.dx || op.dy), 'a patch needs where to take the clean part from (dx, dy)')
  const dx = Math.round(clamp(op.dx, -doc.width, doc.width))
  const dy = Math.round(clamp(op.dy, -doc.height, doc.height))
  let area = null
  let outline = null
  if (hasRect(op)) {
    const rect = rectIn(doc, op)
    area = boxMask(doc, rect)
    outline = rectOutline(rect)
  } else {
    area = selMask(doc, helpers)
    need(anySet(area), 'select what to patch first')
    const b = maskBounds(area, doc.width, doc.height)
    outline = b ? rectOutline(b) : []
  }
  // her hand drags the patch onto the clean part
  await play(helpers, [...outline, ...outline.map(([x, y]) => [x + dx, y + dy])], 'patch')
  const next = cloneDoc(doc)
  const layer = imageTarget(next, op.layer, helpers)
  const undo = await onLayerPixels(doc, layer, helpers, (px) => {
    const hole = docToLayer(area, doc.width, doc.height, layer, px.width, px.height)
    need(anySet(hole), 'that area is not on the picture')
    return out3(fn(px, hole, { dx, dy }), px).pixels
  })
  return { doc: next, undo, label: 'Patched' }
}

// ---------- v3: quick and object select ----------

async function composite3Of(doc, helpers) {
  if (typeof helpers.composite3 === 'function') {
    const px = await helpers.composite3(doc)
    need(px && px.data, 'the picture could not be read')
    return px
  }
  return compositeOf(doc, helpers)
}

async function selectQuick(doc, quick, helpers, E3) {
  const fn = engineFn(E3, 'quickSelect')
  const list = Array.isArray(quick?.strokes) ? quick.strokes.slice(0, 50) : []
  const strokes = list.map((s) => {
    const points = docPoints(doc, Array.isArray(s) ? s : s?.points, 500)
    need(points.length >= 1, 'a quick-select stroke needs points')
    return { points, size: Math.round(clamp(isNum(s?.size) ? s.size : 30, 2, 500)) }
  })
  need(strokes.length >= 1, 'quick select needs strokes')
  // mode 'add' / 'subtract' works on the selection there is; without a mode it starts afresh
  const mode = quick.mode === 'add' || quick.mode === 'subtract' ? quick.mode : null
  const prevMask = mode ? selMask(doc, helpers) : null
  if (mode === 'subtract') need(anySet(prevMask), 'there is no selection to take from')
  for (const s of strokes) await play(helpers, s.points, 'select')
  const comp = await composite3Of(doc, helpers)
  const mask = fn(comp, strokes, { add: mode !== 'subtract', prevMask })
  need(anySet(mask), 'nothing was selected')
  return mask
}

async function selectObject(doc, box, helpers, E3) {
  const fn = engineFn(E3, 'objectSelect')
  const rect = rectIn(doc, box || {})
  await play(helpers, rectOutline(rect), 'select')
  const comp = await composite3Of(doc, helpers)
  const mask = fn(comp, rect, {})
  need(anySet(mask), "I couldn't find an object there")
  return mask
}

// ---------- v3: guides and artboards ----------

function applyGuide(doc, op) {
  const axis = op.axis === 'horizontal' ? 'h' : op.axis === 'vertical' ? 'v' : op.axis
  need(GUIDE_AXES.includes(axis), 'a guide is h or v')
  need(isNum(op.at), 'a guide needs where it sits (at)')
  const at = Math.round(clamp(op.at, 0, axis === 'h' ? doc.height : doc.width))
  const next = cloneDoc(doc)
  next.guides = next.guides || []
  need(!next.guides.some((g) => g.axis === axis && g.at === at), 'that guide is already there')
  need(next.guides.length < MAX_GUIDES, 'there are too many guides')
  next.guides.push({ axis, at })
  return { doc: next, undo: { kind: 'removeGuide', axis, at }, label: `Guide at ${at}` }
}

function applyGuidesClear(doc) {
  const before = Array.isArray(doc.guides) ? doc.guides.map((g) => ({ ...g })) : []
  need(before.length, 'there are no guides')
  const next = cloneDoc(doc)
  next.guides = []
  return { doc: next, undo: { kind: 'guides', before }, label: 'Guides cleared' }
}

function presetsOf(E3) {
  const P = E3 && E3.artboardPresets
  const list = typeof P === 'function' ? P() : P
  if (Array.isArray(list)) return list
  return list && typeof list === 'object' ? Object.entries(list).map(([key, v]) => ({ key, ...v })) : []
}

function findPreset(E3, want) {
  const w = String(want).trim().toLowerCase()
  return presetsOf(E3).find((p) => [p.key, p.id, p.name].some((n) => typeof n === 'string' && n.toLowerCase() === w)) || null
}

const cleanFileName = (s) => String(s ?? '').replace(/[^A-Za-z0-9 _.()-]/g, '-').replace(/^[\s.-]+|[\s.]+$/g, '').slice(0, 60)

function applyArtboard(doc, op, E3) {
  let w = op.w
  let h = op.h
  let name = typeof op.name === 'string' ? op.name.trim() : ''
  if (op.preset != null && op.preset !== '') {
    const p = findPreset(E3, op.preset)
    need(p, `there is no artboard preset called ${op.preset}`)
    w = isNum(w) ? w : p.w ?? p.width
    h = isNum(h) ? h : p.h ?? p.height
    name = name || p.name || String(op.preset)
  }
  need(isNum(w) && isNum(h), 'an artboard needs a width and a height')
  w = Math.round(clamp(w, 1, MAX_SIDE))
  h = Math.round(clamp(h, 1, MAX_SIDE))
  const next = cloneDoc(doc)
  next.artboards = next.artboards || []
  need(next.artboards.length < MAX_ARTBOARDS, 'there are too many artboards')
  const used = new Set(next.artboards.map((a) => a.id))
  let n = next.artboards.length + 1
  while (used.has(`artboard-${n}`)) n++
  // side by side, to the right of the last one
  const x = next.artboards.reduce((m, a) => Math.max(m, (a.x || 0) + (a.w || 0) + 100), 0)
  const board = { id: `artboard-${n}`, name: (name || `Artboard ${n}`).slice(0, 60), x, y: 0, w, h }
  next.artboards.push(board)
  return { doc: next, undo: { kind: 'removeArtboard', id: board.id }, label: `Artboard ${w}×${h}` }
}

// ---------- v3: export, actions and batch ----------

const NONE = (note) => ({ kind: 'none', note })
const plural = (n, one, many = `${one}s`) => `${n} ${n === 1 ? one : many}`

async function applyExport(doc, op, helpers) {
  needHelpers(helpers, ['exportArtboards', 'exportMany'])
  const format = op.format === 'jpg' ? 'jpeg' : op.format == null ? 'png' : op.format
  need(EXPORT_FORMATS.includes(format), 'export is png, jpeg or webp')
  let sizes = null
  if (Array.isArray(op.sizes) && op.sizes.length) {
    sizes = op.sizes.slice(0, MAX_EXPORTS).map((s) => {
      need(isNum(s?.w) && isNum(s?.h), 'each export size needs a width and a height')
      const w = Math.round(clamp(s.w, 1, MAX_SIDE))
      const h = Math.round(clamp(s.h, 1, MAX_SIDE))
      return { w, h, name: cleanFileName(s.name) || `${w}x${h}` }
    })
  }
  const artboards = op.artboards === true
  need(sizes || artboards, 'say which sizes to export, or the artboards')
  if (artboards) need(Array.isArray(doc.artboards) && doc.artboards.length, 'there are no artboards to export')
  const made = await helpers.exportArtboards(doc, sizes, { artboards, format })
  const files = (Array.isArray(made) ? made : []).slice(0, MAX_EXPORTS * 2).map((f) => ({ ...f, name: cleanFileName(f?.name) || 'picture', format: EXPORT_FORMATS.includes(f?.format) ? f.format : format }))
  need(files.length, 'nothing was exported')
  const folder = await helpers.exportMany(files)
  need(folder, 'the export was cancelled')
  return { doc, undo: { ...NONE('An export cannot be taken back - the files stay in the folder.'), folder }, label: `Exported ${plural(files.length, 'file')}` }
}

function actionsOf(helpers) {
  need(helpers.actions && typeof helpers.actions.get === 'function', 'the editor has no actions yet')
  const list = helpers.actions.get()
  return Array.isArray(list) ? list : []
}

function findAction(helpers, name) {
  need(typeof name === 'string' && name.trim(), 'which action?')
  const want = name.trim().toLowerCase()
  const found = actionsOf(helpers).find((a) => String(a?.name || '').trim().toLowerCase() === want)
  need(found && Array.isArray(found.steps), `there is no action called ${name.trim()}`)
  return found
}

async function applyRunAction(doc, op, helpers, E, E3) {
  const depth = helpers.actionDepth || 0
  need(depth < MAX_DEPTH, 'that action runs itself too deep')
  const action = findAction(helpers, op.name)
  const steps = action.steps.slice(0, MAX_STEPS)
  need(steps.length, `the action ${action.name} has no steps`)
  const inner = { ...helpers, actionDepth: depth + 1 }
  const undos = []
  let current = doc
  for (let i = 0; i < steps.length; i++) {
    const step = steps[i]
    need(!NOT_IN_ACTIONS.includes(step?.op), `an action cannot ${step?.op}`)
    try {
      const out = await applyOp(current, step, inner, E, E3)
      undos.push(out.undo)
      current = out.doc
    } catch (err) {
      throw new Error(`step ${i + 1} of ${action.name} (${step?.op ?? '?'}) failed: ${err?.message ?? err}`)
    }
  }
  return { doc: current, undo: { kind: 'sequence', undos }, label: `Ran ${action.name} (${plural(undos.length, 'step')})` }
}

function applyRecordAction(doc, op, helpers) {
  const name = typeof op.name === 'string' ? op.name.trim().slice(0, 60) : ''
  need(name, 'an action needs a name')
  need(helpers.actions && typeof helpers.actions.set === 'function', 'the editor has no actions yet')
  const mine = (Array.isArray(op.steps) ? op.steps : Array.isArray(helpers.herOps) ? helpers.herOps : [])
    .filter((s) => s && typeof s === 'object' && typeof s.op === 'string' && !NOT_IN_ACTIONS.includes(s.op))
  const count = Math.round(clamp(isNum(op.count) ? op.count : mine.length, 1, MAX_STEPS))
  const steps = json(mine.slice(-count))
  need(steps.length, 'there are no steps of mine to record yet')
  const list = actionsOf(helpers)
  const before = list.find((a) => String(a?.name || '').trim().toLowerCase() === name.toLowerCase()) || null
  helpers.actions.set([...list.filter((a) => a !== before), { name, steps }])
  return { doc, undo: { kind: 'action', name, before: before ? json(before) : null }, label: `Recorded ${name} (${plural(steps.length, 'step')})` }
}

async function applyBatch(doc, op, helpers) {
  needHelpers(helpers, ['batchFolder'])
  const action = findAction(helpers, op.action)
  const folder = typeof op.folder === 'string' ? op.folder.trim() : ''
  need(folder, 'which folder?')
  const count = await helpers.batchFolder({ action: action.name, folder, steps: json(action.steps) })
  const n = isNum(count) ? Math.max(0, Math.round(count)) : 0
  return { doc, undo: NONE('A batch cannot be taken back - the originals were never touched.'), label: `Batched ${plural(n, 'picture')} into PHOTO out` }
}

// ---------- the doors ----------

// One op on the doc -> { doc, undo, label }; never changes the input. `engines` defaults to engine2,
// `engines3` to engine3.
export async function applyOp(doc, op, helpers = {}, engines = engine2, engines3 = engine3) {
  need(op && typeof op === 'object' && typeof op.op === 'string', 'the op is not readable')
  const h = helpers || {}
  const E = engines || engine2
  const E3 = engines3 || engine3
  switch (op.op) {
    case 'adjust': return applyAdjust(doc, op)
    case 'text': return applyText(doc, op)
    case 'layer': return applyLayer(doc, op, E3)
    case 'select': return applySelect(doc, op, h, E, E3)
    case 'curves': return applyCurves(doc, op, h)
    case 'levels': return applyLevels(doc, op, h)
    case 'hsl': return applyHsl(doc, op, h)
    case 'filter': return FILTERS3.includes(op.name) ? applyFilter3(doc, op, h, E3) : applyFilter(doc, op, h, E)
    case 'brush': return applyBrush(doc, op, h)
    case 'gradient': return applyGradient(doc, op, h, E)
    case 'shape': return applyShape(doc, op, h, E)
    case 'transform': return applyLayerTransform(doc, op, h, E)
    case 'mask': return applyMaskOp(doc, op, h, E)
    case 'heal': return applyHeal(doc, op, h, E)
    case 'fill': return applyFill(doc, op, h, E)
    case 'erase': return applyErase(doc, op, h, E)
    case 'remove_background': return applyRemoveBackground(doc, op, h, E)
    case 'replace_sky': return applyReplaceSky(doc, op, h, E)
    case 'generate_layer': return applyGenerateLayer(doc, op, h)
    // v3
    case 'group': return applyGroup(doc, op)
    case 'ungroup': return applyUngroup(doc, op)
    case 'clip': return applyClip(doc, op, h)
    case 'blend': return applyBlend(doc, op, h, E3)
    case 'style': return applyStyle(doc, op, h, E3)
    case 'adjustment': return applyAdjustment3(doc, op, h, E3)
    case 'liquify': return applyLiquify(doc, op, h, E3)
    case 'warp': return applyWarp(doc, op, h, E3)
    case 'perspective': return applyPerspective(doc, op, h, E3)
    case 'dodge':
    case 'burn':
    case 'sponge':
    case 'smudge':
    case 'blur_brush':
    case 'sharpen_brush': return applyRetouch(doc, op, h, E3)
    case 'spot_heal': return applySpotHeal(doc, op, h, E3)
    case 'patch': return applyPatch(doc, op, h, E3)
    case 'guide': return applyGuide(doc, op)
    case 'guides_clear': return applyGuidesClear(doc)
    case 'artboard': return applyArtboard(doc, op, E3)
    case 'export_as': return applyExport(doc, op, h)
    case 'run_action': return applyRunAction(doc, op, h, E, E3)
    case 'record_action': return applyRecordAction(doc, op, h)
    case 'batch': return applyBatch(doc, op, h)
    default:
      if (TRANSFORMS.includes(op.op)) return applyDocTransform(doc, op, h)
      throw new Error(`I don't know "${op.op}"`)
  }
}

// Undo one pill on its own -> { doc, dropAfter, note? }. A snapshot brings back the
// whole doc, so the view drops the pills after it. `helpers` (optional) lets an action's
// pill take its recording back (helpers.actions); a pill that cannot be undone (an
// export, a batch) gives the doc back unchanged with a note to show.
export function undoPill(doc, pill, helpers = {}) {
  const undo = pill && pill.undo ? pill.undo : pill
  if (!undo || typeof undo !== 'object') return { doc, dropAfter: false }
  if (undo.kind === 'snapshot') return { doc: cloneDoc(undo.before), dropAfter: true }
  if (undo.kind === 'none') return { doc, dropAfter: false, note: undo.note || 'That cannot be taken back.' }
  if (undo.kind === 'sequence') {
    // her action's steps, undone last to first
    let current = doc
    let dropAfter = false
    const notes = []
    for (const u of [...(undo.undos || [])].reverse()) {
      const r = undoPill(current, { undo: u }, helpers)
      current = r.doc
      dropAfter = dropAfter || r.dropAfter
      if (r.note) notes.push(r.note)
    }
    return notes.length ? { doc: current, dropAfter, note: notes.join(' ') } : { doc: current, dropAfter }
  }
  if (undo.kind === 'action') {
    const a = helpers && helpers.actions
    if (!a || typeof a.get !== 'function' || typeof a.set !== 'function') return { doc, dropAfter: false, note: 'The action list is not open, so the recording stays.' }
    const want = String(undo.name || '').trim().toLowerCase()
    const rest = (a.get() || []).filter((x) => String(x?.name || '').trim().toLowerCase() !== want)
    a.set(undo.before ? [...rest, undo.before] : rest)
    return { doc, dropAfter: false }
  }

  const next = cloneDoc(doc)
  const layer = undo.id != null ? next.layers.find((l) => l.id === undo.id) : null
  if (undo.kind === 'adjust') {
    for (const [key, value] of Object.entries(undo.before || {})) next.adjust[key] = value
  } else if (undo.kind === 'removeLayer') {
    next.layers = next.layers.filter((l) => l.id !== undo.id)
  } else if (undo.kind === 'layerFields') {
    if (layer) Object.assign(layer, undo.before || {})
  } else if (undo.kind === 'layerPng') {
    if (layer) {
      layer.png = undo.before
      if (isNum(undo.x)) layer.x = undo.x
      if (isNum(undo.y)) layer.y = undo.y
    }
  } else if (undo.kind === 'layerMask') {
    if (layer) layer.mask = undo.before ?? null
  } else if (undo.kind === 'selection') {
    next.selection = copySelection(undo.before)
  } else if (undo.kind === 'layerStyle') {
    if (layer) {
      const styles = { ...(layer.styles || {}) }
      if (undo.before) styles[undo.effect] = json(undo.before)
      else delete styles[undo.effect]
      layer.styles = styles
    }
  } else if (undo.kind === 'ungroup') {
    // the group goes; its layers return to their old parents and places
    const moved = new Set(groupIdsOf(next, undo.id))
    next.layers = next.layers.filter((l) => l.id !== undo.id)
    for (const l of next.layers) {
      if (l.parent !== undo.id) continue
      const was = undo.parents ? undo.parents[l.id] : null
      if (was) l.parent = was
      else delete l.parent
    }
    next.layers = restoreOrder(next.layers, undo.order || [], moved)
  } else if (undo.kind === 'regroup') {
    if (undo.layer && !next.layers.some((l) => l.id === undo.layer.id)) {
      const kids = new Set(undo.children || [])
      for (const l of next.layers) if (kids.has(l.id)) l.parent = undo.layer.id
      const at = undo.below == null ? 0 : next.layers.findIndex((l) => l.id === undo.below) + 1
      next.layers.splice(at, 0, copyLayer(undo.layer))
    }
  } else if (undo.kind === 'removeGuide') {
    const i = (next.guides || []).findIndex((g) => g.axis === undo.axis && g.at === undo.at)
    if (i >= 0) next.guides.splice(i, 1)
  } else if (undo.kind === 'guides') {
    const now = next.guides || []
    const back = (undo.before || []).filter((g) => !now.some((n) => n.axis === g.axis && n.at === g.at)).map((g) => ({ ...g }))
    next.guides = [...back, ...now]
  } else if (undo.kind === 'removeArtboard') {
    next.artboards = (next.artboards || []).filter((a) => a.id !== undo.id)
  } else if (undo.kind === 'generated') {
    // a v1 pill: its layer and hidden copy go, what it hid shows again
    next.layers = next.layers.filter((l) => l.id !== undo.newId && (!undo.originalId || l.id !== undo.originalId))
    const hidden = new Set(undo.hidden || [])
    for (const l of next.layers) if (hidden.has(l.id)) l.visible = true
  }
  return { doc: next, dropAfter: false }
}

const OP_WORDS = {
  adjust: 'adjust that',
  crop: 'crop',
  rotate: 'rotate',
  flip: 'flip',
  resize: 'resize',
  text: 'add the text',
  layer: 'change the layer',
  select: 'select that',
  curves: 'set the curves',
  levels: 'set the levels',
  hsl: 'change the hue and saturation',
  filter: 'run that filter',
  brush: 'paint that',
  gradient: 'draw the gradient',
  shape: 'draw the shape',
  transform: 'transform the layer',
  mask: 'mask the layer',
  heal: 'heal that',
  fill: 'fill that',
  remove_background: 'remove the background',
  replace_sky: 'do the sky',
  erase: 'erase that',
  generate_layer: 'draw that layer',
  group: 'group those layers',
  ungroup: 'ungroup that',
  clip: 'clip that layer',
  blend: 'change the blend mode',
  style: 'set that layer style',
  adjustment: 'add that adjustment',
  liquify: 'liquify that',
  warp: 'warp the layer',
  perspective: 'change the perspective',
  dodge: 'dodge that',
  burn: 'burn that',
  sponge: 'sponge that',
  smudge: 'smudge that',
  blur_brush: 'blur that',
  sharpen_brush: 'sharpen that',
  spot_heal: 'spot heal that',
  patch: 'patch that',
  guide: 'add the guide',
  guides_clear: 'clear the guides',
  artboard: 'add the artboard',
  export_as: 'export that',
  run_action: 'run that action',
  record_action: 'record that action',
  batch: 'batch that folder',
}

// His message to her -> { say, results }; each op feeds the next, a failed one is skipped with a note.
export async function ask({ doc, flat, message }, api = { photoAsk }, helpers = {}, engines = engine2, engines3 = engine3) {
  const reply = await api.photoAsk({ flat, doc: stripPixels(doc), message })
  // herOps: what she applied in this ask, in order, for record_action
  const h = { generate: api.photoGenerate, ...helpers, herOps: [] }
  let say = typeof reply?.say === 'string' ? reply.say : ''
  const results = []
  let current = doc
  for (const op of Array.isArray(reply?.ops) ? reply.ops : []) {
    try {
      const out = await applyOp(current, op, h, engines, engines3)
      results.push(out)
      current = out.doc
      h.herOps.push(op)
    } catch (err) {
      const what = OP_WORDS[op?.op] || `do "${op?.op ?? 'that'}"`
      const why = err && err.message ? err.message : String(err)
      say = `${say}${say ? ' ' : ''}(I couldn't ${what}: ${why})`
    }
  }
  return { say, results }
}
