// PHOTO's document: { width, height, layers, adjust, selection }. A layer is
// { id, name, kind: 'image' | 'text', visible, opacity (0..100), blend, x, y, text? }; an image
// layer's pixels live beside the document in the page (one canvas per id), and are saved as one PNG
// per layer. Pure, so it can be tested: the geometry of crop, rotate, flip and resize, the layers,
// and the companion's ops checked again here after the core checked them.
import { ADJUST_KEYS, cleanAdjust, neutralAdjust } from './adjust.js'

export const BLENDS = ['normal', 'multiply', 'screen', 'overlay', 'darken', 'lighten']
export const MAX_SIDE = 8000
export const MAX_OPS = 10
export const REDRAWS = ['remove_background', 'replace_sky', 'erase', 'generate_layer']
export const REDRAW_LABELS = {
  remove_background: 'Remove background',
  replace_sky: 'Replace sky',
  erase: 'Erase',
  generate_layer: 'Generate layer',
}

let counter = 0
/** A new layer id: letters, digits and `-`, unique within this run. */
export function newId(prefix = 'l') {
  counter += 1
  return `${prefix}${Date.now().toString(36)}-${counter.toString(36)}`
}

export function newDoc(width, height) {
  return { width: clampSide(width), height: clampSide(height), layers: [], adjust: neutralAdjust(), selection: null }
}

export function clampSide(v) {
  const n = Math.round(Number(v))
  return Number.isFinite(n) ? Math.min(MAX_SIDE, Math.max(1, n)) : 1
}

export function imageLayer(name, { id = newId(), x = 0, y = 0 } = {}) {
  return { id, name, kind: 'image', visible: true, opacity: 100, blend: 'normal', x, y }
}

export function textLayer(value, { id = newId('t'), x = 0, y = 0, size = 48, color = '#ffffff', font = 'system-ui' } = {}) {
  return { id, name: value.split('\n')[0].slice(0, 30) || 'Text', kind: 'text', visible: true, opacity: 100, blend: 'normal', x, y, text: { value, size, color, font } }
}

/** The canvas composite operation for a blend. */
export function compositeOf(blend) {
  return BLENDS.includes(blend) && blend !== 'normal' ? blend : 'source-over'
}

/** The document as saved and as the companion sees it: no pixels, a clean adjust, the selection's box. */
export function docWithoutPixels(doc) {
  return {
    width: doc.width,
    height: doc.height,
    layers: doc.layers.map(({ png, ...layer }) => ({ ...layer })),
    adjust: cleanAdjust(doc.adjust),
    selection: doc.selection ?? null,
  }
}

/** A document read back from a saved doc.json: sizes clamped, every layer's fields made known. */
export function readDoc(raw) {
  const doc = newDoc(raw?.width, raw?.height)
  doc.adjust = cleanAdjust(raw?.adjust)
  doc.layers = (Array.isArray(raw?.layers) ? raw.layers : []).filter((l) => typeof l?.id === 'string' && /^[A-Za-z0-9_-]{1,40}$/.test(l.id)).map((l) => ({
    id: l.id,
    name: String(l.name ?? 'Layer').slice(0, 60),
    kind: l.kind === 'text' ? 'text' : 'image',
    visible: l.visible !== false,
    opacity: clampNum(l.opacity, 0, 100, 100),
    blend: BLENDS.includes(l.blend) ? l.blend : 'normal',
    x: clampNum(l.x, -MAX_SIDE, MAX_SIDE, 0),
    y: clampNum(l.y, -MAX_SIDE, MAX_SIDE, 0),
    ...(l.kind === 'text' ? { text: cleanText(l.text) } : {}),
  }))
  return doc
}

function clampNum(v, lo, hi, fallback) {
  const n = Number(v)
  return Number.isFinite(n) ? Math.min(hi, Math.max(lo, Math.round(n))) : fallback
}

function cleanText(t) {
  return {
    value: String(t?.value ?? '').slice(0, 500),
    size: clampNum(t?.size, 6, 400, 48),
    color: /^#[0-9a-fA-F]{6}$/.test(t?.color ?? '') ? t.color.toLowerCase() : '#ffffff',
    font: typeof t?.font === 'string' && t.font.length <= 60 ? t.font : 'system-ui',
  }
}

/** The document after a crop to `box`: the new size, every layer shifted (no pixel is lost). */
export function cropDoc(doc, box) {
  return {
    ...doc,
    width: clampSide(box.w),
    height: clampSide(box.h),
    layers: doc.layers.map((l) => ({ ...l, x: l.x - box.x, y: l.y - box.y })),
    selection: null,
  }
}

/** Where a layer of size (lw, lh) at (x, y) lands after `turns` quarter turns clockwise in a W x H picture. */
export function turnBox(x, y, lw, lh, W, H, turns) {
  const t = ((turns % 4) + 4) % 4
  if (t === 1) return { x: H - y - lh, y: x }
  if (t === 2) return { x: W - x - lw, y: H - y - lh }
  if (t === 3) return { x: y, y: W - x - lw }
  return { x, y }
}

/** The document after quarter turns: the sides swap on odd turns. `sizes` gives each image
 * layer's pixel size by id (a text layer only moves its corner). */
export function rotateDoc(doc, turns, sizes = {}) {
  const t = ((turns % 4) + 4) % 4
  const odd = t % 2 === 1
  return {
    ...doc,
    width: odd ? doc.height : doc.width,
    height: odd ? doc.width : doc.height,
    layers: doc.layers.map((l) => {
      const s = sizes[l.id] ?? { w: 0, h: 0 }
      return { ...l, ...turnBox(l.x, l.y, s.w, s.h, doc.width, doc.height, t) }
    }),
    selection: null,
  }
}

/** The document mirrored on `axis` ('h' left-right, 'v' top-bottom). */
export function flipDoc(doc, axis, sizes = {}) {
  return {
    ...doc,
    layers: doc.layers.map((l) => {
      const s = sizes[l.id] ?? { w: 0, h: 0 }
      return axis === 'h' ? { ...l, x: doc.width - l.x - s.w } : { ...l, y: doc.height - l.y - s.h }
    }),
    selection: null,
  }
}

/** The document scaled to width x height: positions and text sizes follow. */
export function resizeDoc(doc, width, height) {
  const w = clampSide(width)
  const h = clampSide(height)
  const sx = w / doc.width
  const sy = h / doc.height
  return {
    ...doc,
    width: w,
    height: h,
    layers: doc.layers.map((l) => ({
      ...l,
      x: Math.round(l.x * sx),
      y: Math.round(l.y * sy),
      ...(l.kind === 'text' ? { text: { ...l.text, size: Math.max(6, Math.min(400, Math.round(l.text.size * sy))) } } : {}),
    })),
    selection: null,
  }
}

/** One layer's fields changed (a new object for the document). */
export function changeLayer(doc, id, fields) {
  return { ...doc, layers: doc.layers.map((l) => (l.id === id ? { ...l, ...fields } : l)) }
}

/** The fields of a layer named in `fields`, as they are now (for an undo). */
export function fieldsOf(layer, fields) {
  return Object.fromEntries(Object.keys(fields).map((k) => [k, layer[k]]))
}

/** A layer moved one place up (toward the top, the end of the list) or down. */
export function moveLayer(doc, id, step) {
  const i = doc.layers.findIndex((l) => l.id === id)
  const j = i + step
  if (i < 0 || j < 0 || j >= doc.layers.length) return doc
  const layers = [...doc.layers]
  ;[layers[i], layers[j]] = [layers[j], layers[i]]
  return { ...doc, layers }
}

/** A layer put in at `index` (the end is the top). */
export function insertLayer(doc, layer, index = doc.layers.length) {
  const layers = [...doc.layers]
  layers.splice(Math.max(0, Math.min(index, layers.length)), 0, layer)
  return { ...doc, layers }
}

export function removeLayer(doc, id) {
  return { ...doc, layers: doc.layers.filter((l) => l.id !== id) }
}

/** The image size asked for a redraw (castle-image.mjs's sizes), nearest the picture's shape. */
export function redrawSize(width, height) {
  const r = width / Math.max(1, height)
  return r > 1.2 ? '1536x1024' : r < 0.83 ? '1024x1536' : '1024x1024'
}

// --- the companion's ops, checked again in the page -------------------------------------------

const num = (v, lo, hi) => {
  const n = Number(v)
  return v === null || v === undefined || v === '' || typeof v === 'boolean' || !Number.isFinite(n) ? null : Math.min(hi, Math.max(lo, Math.round(n)))
}
const words = (v, max) => {
  if (typeof v !== 'string') return null
  // eslint-disable-next-line no-control-regex
  const t = v.replace(/[\u0000-\u0009\u000b-\u001f\u007f]/g, '').slice(0, max).trim()
  return t || null
}

function box(op, doc) {
  const x = num(op.x, 0, doc.width - 1)
  const y = num(op.y, 0, doc.height - 1)
  if (x === null || y === null) return null
  const w = num(op.w, 1, doc.width - x)
  const h = num(op.h, 1, doc.height - y)
  return w === null || h === null ? null : { x, y, w, h }
}

/** One op, checked and clamped the same way as the core (photo.rs check_op); null when unusable. */
export function cleanOp(op, doc) {
  if (!op || typeof op !== 'object') return null
  switch (op.op) {
    case 'adjust': {
      const values = {}
      for (const k of ADJUST_KEYS) {
        const v = num(op.values?.[k], -100, 100)
        if (v !== null) values[k] = v
      }
      return Object.keys(values).length ? { op: 'adjust', values, mode: op.mode === 'set' ? 'set' : 'relative' } : null
    }
    case 'crop':
    case 'erase': {
      const b = box(op, doc)
      return b ? { op: op.op, ...b } : null
    }
    case 'rotate': {
      const t = Number(op.turns)
      if (!Number.isFinite(t)) return null
      const turns = ((Math.round(t) % 4) + 4) % 4
      return turns ? { op: 'rotate', turns } : null
    }
    case 'flip':
      return op.axis === 'h' || op.axis === 'v' ? { op: 'flip', axis: op.axis } : null
    case 'resize': {
      const width = num(op.width, 1, MAX_SIDE)
      const height = num(op.height, 1, MAX_SIDE)
      return width && height ? { op: 'resize', width, height } : null
    }
    case 'text': {
      const text = words(op.text, 500)
      if (!text) return null
      return {
        op: 'text',
        text,
        x: num(op.x, 0, doc.width) ?? 0,
        y: num(op.y, 0, doc.height) ?? 0,
        size: num(op.size, 6, 400) ?? 48,
        color: /^#[0-9a-fA-F]{6}$/.test(op.color ?? '') ? op.color.toLowerCase() : '#ffffff',
      }
    }
    case 'layer': {
      if (!doc.layers.some((l) => l.id === op.id)) return null
      const out = { op: 'layer', id: op.id }
      const o = num(op.opacity, 0, 100)
      if (o !== null) out.opacity = o
      if (BLENDS.includes(op.blend)) out.blend = op.blend
      if (typeof op.visible === 'boolean') out.visible = op.visible
      const n = words(op.name, 60)
      if (n) out.name = n.replace(/\n/g, ' ')
      return Object.keys(out).length > 2 ? out : null
    }
    case 'select': {
      if (op.none === true) return { op: 'select', none: true }
      if (op.all === true) return { op: 'select', all: true }
      const b = box(op, doc)
      return b ? { op: 'select', ...b } : null
    }
    case 'remove_background':
      return { op: 'remove_background' }
    case 'replace_sky':
    case 'generate_layer': {
      const prompt = words(op.prompt, 1000)
      return prompt ? { op: op.op, prompt: prompt.replace(/\n/g, ' ') } : null
    }
    default:
      return null
  }
}

/** Every op checked, at most ten, and only the first redraw. */
export function cleanOps(ops, doc) {
  const out = []
  let redrawn = false
  for (const raw of Array.isArray(ops) ? ops : []) {
    const op = cleanOp(raw, doc)
    if (!op) continue
    if (REDRAWS.includes(op.op)) {
      if (redrawn) continue
      redrawn = true
    }
    out.push(op)
    if (out.length === MAX_OPS) break
  }
  return out
}

/** The line a redraw says in the chat before it starts: what it is and about what it costs. */
export function redrawLine(kind, usd) {
  const what = REDRAW_LABELS[kind] ?? 'Redraw'
  const cost = usd ? ` (about $${usd.toFixed(2)} with an OpenAI key)` : ''
  return `${what} is being redrawn with your image credit${cost}. It lands on a new layer above the originals, which are hidden, not lost.`
}
