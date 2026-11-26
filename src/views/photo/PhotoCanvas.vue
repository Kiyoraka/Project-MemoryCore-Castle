<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import {
  addTextLayer,
  applyAdjust,
  cloneDab,
  cropDoc,
  defaultText,
  flipDoc,
  floodSelect,
  hexToRgba,
  isAdjustEmpty,
  makePixels,
  maskBounds,
  maskForSelection,
  maskFromLasso,
  maskOutline,
  pointInPolygon,
  paintCover,
  resizeDoc,
  rgbToHex,
  rotateDoc,
  setLayer,
} from './engine.js'
import { drawGradient, drawShape, heal, histogram, matrixFor, transformPixels } from './engine2.js'
import { blurBrush, burn, compose3, dodge, objectSelect, patch, quickSelect, sharpenBrush, smudge, snapBox as snapBox3, snapTo, sponge, spotHeal } from './engine3.js'
import {
  addFullLayer,
  addPath,
  descendantIds,
  hasStyles,
  isGroup,
  lineStart,
  presetDab,
  pressured,
  snapLines,
  transformBounds,
  wrapText,
  canvasOffset,
  localMask,
  rotateAround,
  scaleFromCorner,
  setMask,
  shiftMask,
  startTransform,
  transformHit,
} from './viewHelpers.js'
import CompanionCursor from './CompanionCursor.vue'
import TransformBox from './TransformBox.vue'

// PHOTO's canvas v3: every layer composited (engine3.compose3 - groups, clipping, layer styles, 27 blend
// modes - when any of that, masks or adjustment layers are in play; the GPU's own blend for plain layers),
// then the global adjust, shown
// fitted with zoom (Ctrl+wheel, the zoom tool) and pan (Space+drag, the hand, middle button, wheel).
// While something moves it renders at a lighter size; at rest at screen resolution (up to the
// document's own). Every finished edit goes out as change(doc, label).
const props = defineProps({
  doc: { type: Object, default: null },
  selectedId: { type: String, default: '' },
  tool: { type: String, default: 'move' },
  brush: { type: Object, default: () => ({ size: 24, hardness: 0.7, opacity: 1 }) },
  primary: { type: String, default: '#ffffff' },
  secondary: { type: String, default: '#000000' },
  tolerance: { type: Number, default: 32 },
  active: { type: Boolean, default: true },
  editMask: { type: Boolean, default: false },
  viewMask: { type: String, default: '' },
  gradient: { type: Object, default: () => ({ kind: 'linear', stops: [{ at: 0, color: '#ffffff' }, { at: 1, color: '#000000' }] }) },
  shape: { type: Object, default: () => ({ kind: 'rect', fill: '#ffffff', fillOn: true, stroke: '#000000', strokeOn: false, width: 4 }) },
  snap: { type: Boolean, default: true }, // move, marquee, crop, transform snap to guides, grid and edges
  channel: { type: String, default: '' }, // '' | 'r' | 'g' | 'b' | 'alpha' - one channel shown alone
  quickMask: { type: Boolean, default: false }, // the brush paints the quick mask (the parent starts / ends it)
  activePath: { type: Object, default: null }, // a kept path shown over the picture
  companion: { type: String, default: 'Your companion' }, // the name beside the companion's pointer
})
// change(doc, label, meta?) - meta.op: the step as her op, for actions
const emit = defineEmits(['change', 'select', 'colour', 'problem', 'transform', 'view'])

const MODES = { normal: 'source-over', multiply: 'multiply', screen: 'screen', overlay: 'overlay', darken: 'darken', lighten: 'lighten' }
const PAINTING = ['brush', 'eraser', 'clone', 'heal']
// brushes that paint a path and change the layer once it is let go (engine3)
const RETOUCH = { dodge, burn, sponge, smudge, blurBrush, sharpenBrush, spotHeal }
const RETOUCH_OPS = { dodge: 'dodge', burn: 'burn', sponge: 'sponge', smudge: 'smudge', blurBrush: 'blur_brush', sharpenBrush: 'sharpen_brush', spotHeal: 'spot_heal' }
const RETOUCH_LABELS = { dodge: 'Dodged', burn: 'Burned', sponge: 'Sponged', smudge: 'Smudged', blurBrush: 'Blurred', sharpenBrush: 'Sharpened', spotHeal: 'Spot healed' }
const PATHED = [...Object.keys(RETOUCH), 'heal', 'quickSelect']
const MAX_REST = 16e6 // pixels at rest
const MAX_QUICK = 0.8e6 // pixels while something moves

const stage = ref(null)
const display = ref(null)
const textBox = ref(null)
const companionCursor = ref(null)
const stageSize = ref({ w: 800, h: 600 })
const zoom = ref(1)
const pan = ref({ x: 0, y: 0 })
const spaceDown = ref(false)
const altDown = ref(false)
const panning = ref(null)
const hover = ref(null)
const temp = ref(null) // a selection being drawn: { kind: 'rect', x0, y0, x1, y1 } | { kind: 'lasso', points }
const cropRect = ref(null)
const cropDrag = ref(null)
const editing = ref(null) // { id | null, x, y, value, text }
const cloneSource = ref(null)
const healSource = ref(null)
const maskPath = ref('')
const drag = ref(null)
const gradLine = ref(null) // { from, to }
const shapeDrag = ref(null) // { kind, x0, y0, x1, y1, square }
const poly = ref(null) // { points } - a polygon shape being clicked
const pen = ref(null) // { points }
const healPath = ref(null) // { points, tool, alt } in document units - heal, the retouch brushes, quick select
const textDrag = ref(null) // { x0, y0, x1, y1 } - a paragraph box being dragged with the text tool
const patchSel = ref(null) // { points } - the patch tool's area, waiting to be dragged onto a clean part
const patchDrag = ref(null) // { from, dx, dy }
const quickView = ref(null)
const xf = ref(null) // { id, t, ox, oy, start?, grab? } - free transform
const working = ref(false)

// png data URL -> decoded canvas, oldest first; the live document's own are never dropped
const cache = new Map()
const decoding = new Map()
const scaledImages = new Map() // png -> { w, h, pixels } at the last size asked
const fullMasks = new Map() // mask png -> Uint8Array (document sized)
const scaledMasks = new Map() // mask png -> { w, h, data }
const textPixelsCache = new Map() // layer id -> { sig, pixels }
let live = null // { id, canvas } while a stroke paints a working copy of a layer
let liveMask = null // { id, canvas } while a stroke paints a layer's mask
let stroke = null
let cloneOffset = null
let healOffset = null
let preview = null // { id, fn(pixels, scale, mask) } - a filter dialog's live look
let quick = null // { canvas } - the quick mask: white, its alpha = how selected
let version = 0 // bumps whenever something not in the doc changes the picture
const selCache = { selection: null, mask: null }
const maskByPng = new Map() // mask png we made -> its values, so the selection can be read at once
const scratch = document.createElement('canvas')
const scratchCtx = scratch.getContext('2d', { willReadFrequently: true })
const comp = document.createElement('canvas')
const compCtx = comp.getContext('2d', { willReadFrequently: true })
const shownScale = ref(1)

const W = computed(() => props.doc?.width || 1)
const H = computed(() => props.doc?.height || 1)
const fit = computed(() => Math.min((stageSize.value.w - 32) / W.value, (stageSize.value.h - 32) / H.value, 4) || 1)
const scale = computed(() => fit.value * zoom.value)
const boardPos = computed(() => ({
  x: (stageSize.value.w - W.value * scale.value) / 2 + pan.value.x,
  y: (stageSize.value.h - H.value * scale.value) / 2 + pan.value.y,
}))
const boardStyle = computed(() => ({
  width: `${W.value}px`,
  height: `${H.value}px`,
  left: `${boardPos.value.x}px`,
  top: `${boardPos.value.y}px`,
  transform: `scale(${scale.value})`,
}))
const px = computed(() => 1 / scale.value) // one screen pixel in document units

function makeCanvas(w, h) {
  const c = document.createElement('canvas')
  c.width = Math.max(1, Math.round(w))
  c.height = Math.max(1, Math.round(h))
  return c
}

const asPixels = (img) => ({ data: img.data, width: img.width, height: img.height })

function readCanvas(c) {
  return asPixels(c.getContext('2d', { willReadFrequently: true }).getImageData(0, 0, c.width, c.height))
}

function canvasOf(pixels) {
  const c = makeCanvas(pixels.width, pixels.height)
  c.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(pixels.data), pixels.width, pixels.height), 0, 0)
  return c
}

// A canvas drawn at another size -> its pixels.
function resample(src, w, h) {
  if (scratch.width !== w || scratch.height !== h) {
    scratch.width = w
    scratch.height = h
  } else scratchCtx.clearRect(0, 0, w, h)
  scratchCtx.imageSmoothingQuality = 'high'
  scratchCtx.drawImage(src, 0, 0, w, h)
  return asPixels(scratchCtx.getImageData(0, 0, w, h))
}

// ---------- decoding ----------

function decode(src) {
  if (decoding.has(src)) return decoding.get(src)
  const job = new Promise((resolve, reject) => {
    const img = new Image()
    img.onload = () => {
      const c = makeCanvas(img.naturalWidth, img.naturalHeight)
      c.getContext('2d').drawImage(img, 0, 0)
      resolve(c)
    }
    img.onerror = () => reject(new Error('A layer picture could not be read.'))
    img.src = src
  }).finally(() => decoding.delete(src))
  decoding.set(src, job)
  return job
}

function remember(png, canvas) {
  cache.delete(png)
  cache.set(png, canvas)
}

async function prepare(doc) {
  const wanted = []
  for (const l of doc.layers) {
    if (l.kind === 'image' && l.png && !cache.has(l.png)) wanted.push(l.png)
    if (l.mask && !cache.has(l.mask)) wanted.push(l.mask)
  }
  await Promise.all(
    wanted.map(async (png) => {
      try {
        remember(png, await decode(png))
      } catch {
        remember(png, makeCanvas(1, 1))
      }
    }),
  )
}

function prune() {
  if (cache.size <= 32) return
  const keep = new Set([props.doc?.selection?.png])
  for (const l of props.doc?.layers || []) {
    if (l.png) keep.add(l.png)
    if (l.mask) keep.add(l.mask)
  }
  for (const key of cache.keys()) {
    if (cache.size <= 32) break
    if (!keep.has(key)) {
      cache.delete(key)
      scaledImages.delete(key)
      fullMasks.delete(key)
      scaledMasks.delete(key)
      maskByPng.delete(key)
    }
  }
}

// A mask canvas (grey, or white with alpha) -> w*h values, value = grey * alpha.
function maskValues(canvas, w, h) {
  if (scratch.width !== w || scratch.height !== h) {
    scratch.width = w
    scratch.height = h
  }
  scratchCtx.fillStyle = '#000'
  scratchCtx.fillRect(0, 0, w, h)
  scratchCtx.imageSmoothingQuality = 'high'
  scratchCtx.drawImage(canvas, 0, 0, w, h)
  const data = scratchCtx.getImageData(0, 0, w, h).data
  const out = new Uint8Array(w * h)
  for (let j = 0; j < out.length; j++) out[j] = data[j * 4]
  return out
}

function fullMask(png, w, h) {
  const hit = fullMasks.get(png)
  if (hit && hit.length === w * h) return hit
  const c = cache.get(png)
  if (!c) return null
  const m = maskValues(c, w, h)
  fullMasks.set(png, m)
  return m
}

function scaledMask(png, w, h) {
  const hit = scaledMasks.get(png)
  if (hit && hit.w === w && hit.h === h) return hit.data
  const c = cache.get(png)
  if (!c) return null
  const data = maskValues(c, w, h)
  scaledMasks.set(png, { w, h, data })
  return data
}

function scaledImage(png, src, w, h) {
  const hit = scaledImages.get(png)
  if (hit && hit.w === w && hit.h === h) return hit.pixels
  const pixels = resample(src, w, h)
  scaledImages.set(png, { w, h, pixels })
  return pixels
}

// ---------- text ----------

function textFont(t) {
  return `${t.weight || 400} ${t.size || 32}px ${t.font || 'sans-serif'}`
}

// The lines of a text layer: point text breaks at \n; paragraph text (textBox) wraps inside its box, aligned,
// with its line height and tracking.
function textLayout(layer) {
  const t = layer.text || {}
  const size = t.size || 32
  const box = layer.textBox || null
  const lh = size * (box?.lineHeight ?? 1.2)
  const tracking = box?.tracking || 0
  compCtx.font = textFont(t)
  if ('letterSpacing' in compCtx) compCtx.letterSpacing = '0px'
  const widthOf = (str) => compCtx.measureText(str).width
  const value = String(t.value ?? '')
  const raw = box ? wrapText(value, box.w, widthOf, tracking) : value.split('\n')
  const lines = raw.map((text) => ({ text, w: widthOf(text) + tracking * Math.max(0, text.length - 1) }))
  const w = box ? box.w : Math.ceil(Math.max(1, ...lines.map((l) => l.w)))
  const h = Math.ceil(Math.max(box?.h || 0, lines.length * lh))
  return { lines, w, h, lh, tracking, align: box?.align || 'left' }
}

function drawText(ctx, layer, x, y) {
  const t = layer.text || {}
  const L = textLayout(layer)
  ctx.font = textFont(t)
  ctx.textBaseline = 'top'
  ctx.fillStyle = t.color || '#ffffff'
  const spaced = 'letterSpacing' in ctx
  if (spaced) ctx.letterSpacing = `${L.tracking}px`
  L.lines.forEach((line, i) => ctx.fillText(line.text, x + lineStart(L.align, L.w, line.w), y + i * L.lh))
  if (spaced) ctx.letterSpacing = '0px'
}

function measure(layer) {
  if (layer.kind === 'image') {
    const c = cache.get(layer.png)
    return { w: c?.width || 0, h: c?.height || 0 }
  }
  if (layer.kind !== 'text') return { w: 0, h: 0 }
  const L = textLayout(layer)
  return { w: Math.ceil(L.w), h: L.h }
}

function textCanvas(layer, s) {
  const { w, h } = measure(layer)
  const c = makeCanvas(w * s + 2, h * s + 2)
  const ctx = c.getContext('2d')
  ctx.scale(s, s)
  drawText(ctx, layer, 0, 0)
  return c
}

function textPixels(layer, s) {
  const sig = `${JSON.stringify(layer.text)}|${JSON.stringify(layer.textBox || null)}|${s}`
  const hit = textPixelsCache.get(layer.id)
  if (hit?.sig === sig) return hit.pixels
  const pixels = readCanvas(textCanvas(layer, s))
  textPixelsCache.set(layer.id, { sig, pixels })
  return pixels
}

// ---------- the selection ----------

async function selectionMask(doc = props.doc) {
  const sel = doc?.selection
  if (!sel) return null
  if (selCache.selection === sel) return selCache.mask
  let mask
  if (sel.kind === 'mask') {
    mask = knownMask(sel.png, doc.width, doc.height)
    if (!mask) {
      const c = await decode(sel.png)
      mask = maskValues(c, doc.width, doc.height)
      maskByPng.set(sel.png, mask)
    }
  } else mask = maskForSelection(sel, doc.width, doc.height)
  selCache.selection = sel
  selCache.mask = mask
  return mask
}

function knownMask(png, w, h) {
  const hit = maskByPng.get(png)
  if (hit && hit.length === w * h) return hit
  const c = cache.get(png)
  if (!c) return null
  const m = maskValues(c, w, h)
  maskByPng.set(png, m)
  return m
}

// The same, at once (her ops ask for it without waiting): a mask selection is read from the masks
// this canvas made or has decoded already.
function selectionMaskNow(doc = props.doc) {
  const sel = doc?.selection
  if (!sel) return null
  if (selCache.selection === sel) return selCache.mask
  const mask = sel.kind === 'mask' ? knownMask(sel.png, doc.width, doc.height) : maskForSelection(sel, doc.width, doc.height)
  if (mask) {
    selCache.selection = sel
    selCache.mask = mask
  }
  return mask
}

async function outlineSelection() {
  const doc = props.doc
  const sel = doc?.selection
  if (sel?.kind !== 'mask') {
    maskPath.value = ''
    return
  }
  try {
    const mask = await selectionMask(doc)
    if (props.doc?.selection !== sel) return
    // too busy to outline: its bounds instead
    const b = maskBounds(mask, doc.width, doc.height)
    maskPath.value = maskOutline(mask, doc.width, doc.height) ?? (b ? rectPath(b) : '')
  } catch {
    maskPath.value = ''
  }
}

function rectPath(r) {
  return `M${r.x} ${r.y}h${r.w}v${r.h}h${-r.w}Z`
}

const antsPath = computed(() => {
  const t = temp.value
  if (t?.kind === 'rect') return rectPath(normRect(t))
  if (t?.kind === 'lasso') return t.points.length > 1 ? `M${t.points.map((p) => p.join(' ')).join('L')}` : ''
  const sel = props.doc?.selection
  if (!sel) return ''
  if (sel.kind === 'rect') return rectPath(sel)
  if (sel.kind === 'lasso') return sel.points?.length > 2 ? `M${sel.points.map((p) => p.join(' ')).join('L')}Z` : ''
  return maskPath.value
})

function clampBox(b) {
  const x0 = Math.max(0, Math.floor(b.x))
  const y0 = Math.max(0, Math.floor(b.y))
  const x1 = Math.min(W.value, Math.ceil(b.x + b.w))
  const y1 = Math.min(H.value, Math.ceil(b.y + b.h))
  return { x: x0, y: y0, w: Math.max(0, x1 - x0), h: Math.max(0, y1 - y0) }
}

function normRect(t) {
  return clampBox({ x: Math.min(t.x0, t.x1), y: Math.min(t.y0, t.y1), w: Math.abs(t.x1 - t.x0), h: Math.abs(t.y1 - t.y0) })
}

function setSelection(selection, label, meta) {
  if (!props.doc) return
  if (!selection && !props.doc.selection) return
  emit('change', { ...props.doc, selection }, label, meta)
}

// A selection mask as v1 keeps it: white with the mask as alpha.
function selectionPng(mask, w, h) {
  const c = makeCanvas(w, h)
  const ctx = c.getContext('2d')
  const img = ctx.createImageData(w, h)
  for (let j = 0; j < mask.length; j++) {
    const i = j * 4
    img.data[i] = img.data[i + 1] = img.data[i + 2] = 255
    img.data[i + 3] = mask[j]
  }
  ctx.putImageData(img, 0, 0)
  const png = c.toDataURL('image/png')
  remember(png, c)
  maskByPng.set(png, mask)
  return png
}

// A layer mask: grey, opaque (white shown).
function maskPngFrom(mask, w, h) {
  const c = makeCanvas(w, h)
  const ctx = c.getContext('2d')
  const img = ctx.createImageData(w, h)
  for (let j = 0; j < mask.length; j++) {
    const i = j * 4
    img.data[i] = img.data[i + 1] = img.data[i + 2] = mask[j]
    img.data[i + 3] = 255
  }
  ctx.putImageData(img, 0, 0)
  const png = c.toDataURL('image/png')
  remember(png, c)
  maskByPng.set(png, mask)
  return png
}

function pngFrom(pixels) {
  const c = canvasOf(pixels)
  const png = c.toDataURL('image/png')
  remember(png, c)
  return png
}

async function magicWand(p) {
  const doc = props.doc
  working.value = true
  try {
    const pixels = await composite(doc)
    const mask = floodSelect(pixels, p.x, p.y, props.tolerance)
    setSelection({ kind: 'mask', png: selectionPng(mask, doc.width, doc.height) }, 'Magic wand')
  } finally {
    working.value = false
  }
}

// ---------- compositing ----------

// Plain: picture and text layers only, no masks, groups, clipping or styles, in a blend the GPU has.
const isPlain = (doc) => doc.layers.every((l) => (l.kind === 'image' || l.kind === 'text') && !l.mask && !l.parent && !l.clipped && !hasStyles(l) && MODES[l.blend || 'normal'])

// Layer styles at the render size: their distances and sizes shrink with the picture.
function scaledStyles(styles, s) {
  if (!styles || s === 1) return styles || undefined
  const out = {}
  for (const [k, v] of Object.entries(styles)) {
    out[k] = { ...v }
    for (const f of ['distance', 'size', 'width']) if (typeof v?.[f] === 'number') out[k][f] = v[f] * s
  }
  return out
}

// What compose3 needs of a layer besides its pixels.
const tree = (l, s) => ({ id: l.id, parent: l.parent, clipped: !!l.clipped, styles: scaledStyles(l.styles, s), opacity: l.opacity, blend: l.blend || 'normal' })

// A hidden layer still goes to compose3 (a clipped layer and a group's members need to know).
const hidden = (l, s) => ({ ...tree(l, s), kind: l.kind, visible: false })

// The layer's source canvas as the picture shows it right now.
function sourceOf(l) {
  if (l.kind !== 'image') return null
  return live?.id === l.id ? live.canvas : cache.get(l.png)
}

function maskCanvasOf(l) {
  if (liveMask?.id === l.id) return liveMask.canvas
  return l.mask ? cache.get(l.mask) : null
}

// The free-transform preview of a layer onto a canvas of w*h (document scaled by s).
function transformedCanvas(src, s, w, h, offset = null) {
  const t = xf.value.t
  const m = matrixFor({ scale: [t.sx, t.sy], rotate: (t.rotate * 180) / Math.PI, cx: t.w / 2, cy: t.h / 2, dx: t.cx - t.w / 2, dy: t.cy - t.h / 2 })
  const c = makeCanvas(w, h)
  const ctx = c.getContext('2d')
  ctx.imageSmoothingQuality = 'high'
  ctx.setTransform(s * m.a, s * m.b, s * m.c, s * m.d, s * m.e, s * m.f)
  if (offset) ctx.translate(-offset.x, -offset.y)
  ctx.drawImage(src, 0, 0)
  return c
}

function transformedMask(l, s, w, h) {
  const mc = maskCanvasOf(l)
  if (!mc) return null
  // the mask is document sized: back to the layer's frame first
  const ox = xf.value.ox
  const oy = xf.value.oy
  const c = transformedCanvas(mc, s, w, h, { x: ox, y: oy })
  return maskValues(c, w, h)
}

const dragOffset = (l) => (drag.value?.ids?.has(l.id) ? { dx: drag.value.dx, dy: drag.value.dy } : { dx: 0, dy: 0 })

// Every layer at scale s (w*h) as compose3 takes them.
async function layersAt(doc, s, w, h) {
  const out = []
  const sel = preview ? await selectionMask(doc) : null
  const selScaled = sel ? maskValues(canvasOf(maskAsPixels(sel, doc.width, doc.height)), w, h) : null
  for (const l of doc.layers) {
    if (!l.visible || !(l.opacity > 0)) {
      out.push(hidden(l, s))
      continue
    }
    const mc = l.mask || liveMask?.id === l.id ? maskCanvasOf(l) : null
    let mask = mc ? (liveMask?.id === l.id ? maskValues(mc, w, h) : scaledMask(l.mask, w, h)) : null
    if (l.kind === 'group') {
      out.push({ ...tree(l, s), kind: 'group', visible: true, mask })
      continue
    }
    if (l.kind === 'adjustment') {
      out.push({ ...tree(l, s), kind: 'adjustment', adjustment: l.adjustment, visible: true, mask })
      continue
    }
    let pixels
    let x
    let y
    if (l.kind === 'image') {
      const src = sourceOf(l)
      if (!src) continue
      if (xf.value?.id === l.id) {
        pixels = readCanvas(transformedCanvas(src, s, w, h))
        if (mc) mask = transformedMask(l, s, w, h)
        x = 0
        y = 0
      } else {
        const pw = Math.max(1, Math.round(src.width * s))
        const ph = Math.max(1, Math.round(src.height * s))
        pixels = live?.id === l.id ? resample(src, pw, ph) : scaledImage(l.png, src, pw, ph)
        const { dx, dy } = dragOffset(l)
        x = Math.round((l.x + dx) * s)
        y = Math.round((l.y + dy) * s)
      }
    } else if (l.kind === 'text') {
      if (editing.value?.id === l.id) {
        out.push(hidden(l, s))
        continue
      }
      pixels = textPixels(l, s)
      const { dx, dy } = dragOffset(l)
      x = Math.round((l.x + dx) * s)
      y = Math.round((l.y + dy) * s)
    } else continue
    if (preview?.id === l.id) {
      const local = selScaled ? localMask(selScaled, w, h, x, y, pixels.width, pixels.height) : null
      try {
        pixels = preview.fn(pixels, s, local) || pixels
      } catch {
        // a preview that fails shows the layer as it is
      }
    }
    out.push({ ...tree(l, s), kind: l.kind, pixels, x, y, visible: true, mask })
  }
  return out
}

// Plain layers through the canvas' own blending (the same W3C formulas), straight onto comp.
function drawPlain(doc, s, w, h) {
  if (comp.width !== w || comp.height !== h) {
    comp.width = w
    comp.height = h
  } else compCtx.clearRect(0, 0, w, h)
  compCtx.imageSmoothingQuality = 'high'
  for (const l of doc.layers) {
    if (!l.visible || !(l.opacity > 0)) continue
    compCtx.globalAlpha = l.opacity
    compCtx.globalCompositeOperation = MODES[l.blend] || 'source-over'
    const { dx, dy } = dragOffset(l)
    if (l.kind === 'image') {
      const src = sourceOf(l)
      if (!src) continue
      if (xf.value?.id === l.id) compCtx.drawImage(transformedCanvas(src, s, w, h), 0, 0)
      else compCtx.drawImage(src, (l.x + dx) * s, (l.y + dy) * s, src.width * s, src.height * s)
    } else if (l.kind === 'text' && editing.value?.id !== l.id) {
      compCtx.save()
      compCtx.scale(s, s)
      drawText(compCtx, l, l.x + dx, l.y + dy)
      compCtx.restore()
    }
  }
  compCtx.globalAlpha = 1
  compCtx.globalCompositeOperation = 'source-over'
}

function maskAsPixels(mask, w, h) {
  const p = makePixels(w, h)
  for (let j = 0; j < mask.length; j++) {
    const i = j * 4
    p.data[i] = p.data[i + 1] = p.data[i + 2] = mask[j]
    p.data[i + 3] = 255
  }
  return p
}

// Plain layers with no global adjust are the GPU's work alone: no need to go lighter while moving.
const heavy = (doc) => !!doc && (!isPlain(doc) || !!preview || !isAdjustEmpty(doc.adjust))

function targetScale(quick) {
  const area = W.value * H.value
  const dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1
  let s = Math.min(1, scale.value * dpr, Math.sqrt(MAX_REST / area))
  if (quick && heavy(props.doc)) s = Math.min(s, Math.sqrt(MAX_QUICK / area))
  return Math.max(s, Math.min(1, 48 / Math.max(W.value, H.value)))
}

let token = 0
let last = null // { layers, w, h, version, viewMask, pixels | null (plain: on comp) }
async function render(quick) {
  const doc = props.doc
  const out = display.value
  if (!doc || !out) return
  const mine = ++token
  await prepare(doc)
  if (mine !== token || doc !== props.doc) return
  const s0 = targetScale(quick)
  const w = Math.max(1, Math.round(doc.width * s0))
  const h = Math.max(1, Math.round(doc.height * s0))
  const s = w / doc.width
  const viewing = props.viewMask && doc.layers.find((l) => l.id === props.viewMask && (l.mask || liveMask?.id === l.id))
  let pixels
  if (viewing) {
    const mc = maskCanvasOf(viewing)
    pixels = maskAsPixels(maskValues(mc, w, h), w, h)
  } else {
    const same = last && last.layers === doc.layers && last.w === w && last.h === h && last.version === version && !last.viewMask
    const plain = isPlain(doc) && !preview
    if (same && last.pixels) pixels = last.pixels
    else if (plain) {
      if (!same) drawPlain(doc, s, w, h)
      pixels = isAdjustEmpty(doc.adjust) ? null : asPixels(compCtx.getImageData(0, 0, w, h))
      last = { layers: doc.layers, w, h, version, viewMask: '', pixels: null }
    } else {
      const layers = await layersAt(doc, s, w, h)
      if (mine !== token || doc !== props.doc) return
      pixels = compose3(layers, w, h)
      last = { layers: doc.layers, w, h, version, viewMask: '', pixels }
    }
    if (pixels && !isAdjustEmpty(doc.adjust)) pixels = applyAdjust(pixels, doc.adjust)
    if (props.channel) pixels = channelPixels(pixels || asPixels(compCtx.getImageData(0, 0, w, h)), props.channel)
  }
  if (mine !== token) return
  if (out.width !== w || out.height !== h) {
    out.width = w
    out.height = h
  }
  const ctx = out.getContext('2d', { willReadFrequently: true })
  if (pixels) ctx.putImageData(new ImageData(pixels.data, w, h), 0, 0)
  else {
    ctx.clearRect(0, 0, w, h)
    ctx.drawImage(comp, 0, 0)
  }
  if (viewing) last = null
  shownScale.value = s
  prune()
}

let frame = 0
let restTimer = 0
// Something changed: a light render on the next frame, the full one once things are still.
function schedule() {
  if (!frame) frame = requestAnimationFrame(() => {
    frame = 0
    render(true)
  })
  clearTimeout(restTimer)
  restTimer = setTimeout(() => {
    if (Math.abs(targetScale(false) - shownScale.value) > 1e-3 || !last) render(false)
  }, 220)
}
function bump() {
  version += 1
  schedule()
}

watch(
  () => props.doc,
  (doc, old) => {
    if (!doc) return
    schedule()
    if (!old || doc.selection !== old.selection) outlineSelection()
    if (old && (doc.width !== old.width || doc.height !== old.height)) {
      cropRect.value = null
      resetView()
    }
    if (xf.value && !doc.layers.some((l) => l.id === xf.value.id)) cancelTransform()
  },
  { immediate: true },
)
watch(() => props.viewMask, bump)
watch(() => props.channel, bump)

// One channel as a grey picture (alpha: how opaque).
function channelPixels(p, ch) {
  const k = { r: 0, g: 1, b: 2, alpha: 3 }[ch]
  if (k === undefined) return p
  const d = p.data
  const o = new Uint8ClampedArray(d.length)
  for (let i = 0; i < d.length; i += 4) {
    const v = k === 3 ? d[i + 3] : d[i + k]
    o[i] = o[i + 1] = o[i + 2] = v
    o[i + 3] = 255
  }
  return { data: o, width: p.width, height: p.height }
}
watch(scale, () => {
  clearTimeout(restTimer)
  restTimer = setTimeout(() => render(false), 160)
})

// ---------- full-size pixels (helpers for the view and for her) ----------

async function pixelsOf(doc, id) {
  if (!doc) return null
  await prepare(doc)
  const l = doc.layers.find((x) => x.id === id)
  if (!l) return null
  if (l.kind === 'image') {
    const c = cache.get(l.png)
    return c ? readCanvas(c) : null
  }
  if (l.kind === 'text') return readCanvas(textCanvas(l, 1))
  return null
}

async function maskOf(doc, id) {
  if (!doc) return null
  const l = doc.layers.find((x) => x.id === id)
  if (!l?.mask) return null
  await prepare(doc)
  return fullMask(l.mask, doc.width, doc.height)
}

async function fullLayers(doc, upto = Infinity) {
  await prepare(doc)
  const out = []
  doc.layers.forEach((l, i) => {
    if (i >= upto) return
    if (!l.visible || !(l.opacity > 0)) return out.push(hidden(l, 1))
    const mask = l.mask ? fullMask(l.mask, doc.width, doc.height) : null
    if (l.kind === 'group') out.push({ ...tree(l, 1), kind: 'group', visible: true, mask })
    else if (l.kind === 'adjustment') out.push({ ...tree(l, 1), kind: 'adjustment', adjustment: l.adjustment, visible: true, mask })
    else if (l.kind === 'image') {
      const c = cache.get(l.png)
      if (c) out.push({ ...tree(l, 1), kind: 'image', pixels: readCanvas(c), x: l.x, y: l.y, visible: true, mask })
    } else if (l.kind === 'text') out.push({ ...tree(l, 1), kind: 'text', pixels: readCanvas(textCanvas(l, 1)), x: l.x, y: l.y, visible: true, mask })
  })
  return out
}

// Every visible layer - groups, clipping, styles and adjustment layers in - without doc.adjust.
async function composite(doc = props.doc, upto = Infinity) {
  if (!doc) return null
  return compose3(await fullLayers(doc, upto), doc.width, doc.height)
}

// The histogram of what an adjustment layer works on (everything under it), at a light size.
async function histogramBelow(id) {
  const doc = props.doc
  if (!doc) return null
  const i = doc.layers.findIndex((l) => l.id === id)
  if (i < 0) return null
  const s = Math.min(1, Math.sqrt(250000 / (doc.width * doc.height)))
  const w = Math.max(1, Math.round(doc.width * s))
  const h = Math.max(1, Math.round(doc.height * s))
  await prepare(doc)
  const below = { ...doc, layers: doc.layers.slice(0, i) }
  const layers = await layersAt(below, w / doc.width, w, h)
  return histogram(compose3(layers, w, h))
}

// ---------- the pointer ----------

function docPoint(event) {
  const r = display.value.getBoundingClientRect()
  return { x: ((event.clientX - r.left) / r.width) * W.value, y: ((event.clientY - r.top) / r.height) * H.value }
}

const selectedLayer = () => props.doc?.layers.find((l) => l.id === props.selectedId) || null

// The pen's pressure (a mouse counts as full).
const pressureOf = (event) => (event.pointerType === 'pen' && event.pressure > 0 ? event.pressure : 1)

// Snapping: guides, grid (when shown with snap on), the canvas and artboard edges, within 6 screen px.
function snapPoint(p) {
  if (!props.snap || !props.doc) return p
  const tol = 6 * px.value
  const lines = snapLines(props.doc, {}, { x: [p.x], y: [p.y] })
  return { ...p, x: snapTo(p.x, lines.xs, tol).value, y: snapTo(p.y, lines.ys, tol).value }
}

// A box moved by (dx, dy) -> the move that lands its nearest edge or centre on a line.
function snapMove(box, dx, dy) {
  if (!props.snap || !props.doc || !box) return { dx, dy }
  const tol = 6 * px.value
  const moved = { x: box.x + dx, y: box.y + dy, w: box.w, h: box.h }
  const lines = snapLines(props.doc, {}, { x: [moved.x, moved.x + box.w], y: [moved.y, moved.y + box.h] })
  const r = snapBox3(moved, lines, tol)
  return { dx: Math.round(r.x - box.x), dy: Math.round(r.y - box.y) }
}

// The layers a move takes along: the layer, or every picture and text layer inside a group.
function movedIds(layer) {
  if (!isGroup(layer)) return [layer.id]
  const inside = descendantIds(props.doc.layers, layer.id)
  return props.doc.layers.filter((l) => inside.has(l.id) && (l.kind === 'image' || l.kind === 'text')).map((l) => l.id)
}

function boundsOf(ids) {
  let box = null
  for (const id of ids) {
    const l = props.doc.layers.find((x) => x.id === id)
    if (!l) continue
    const m = measure(l)
    const b = { x: l.x, y: l.y, w: m.w, h: m.h }
    if (!box) box = b
    else {
      const x1 = Math.max(box.x + box.w, b.x + b.w)
      const y1 = Math.max(box.y + box.h, b.y + b.h)
      box = { x: Math.min(box.x, b.x), y: Math.min(box.y, b.y), w: 0, h: 0 }
      box.w = x1 - box.x
      box.h = y1 - box.y
    }
  }
  return box
}

function onDown(event) {
  if (!props.doc) return
  if (event.button === 1 || (event.button === 0 && (spaceDown.value || props.tool === 'hand'))) {
    panning.value = { sx: event.clientX, sy: event.clientY, px: pan.value.x, py: pan.value.y }
    stage.value.setPointerCapture?.(event.pointerId)
    event.preventDefault()
    return
  }
  if (event.button !== 0) return
  if (editing.value) {
    commitText()
    return
  }
  const p = docPoint(event)
  if (xf.value) {
    const hit = transformHit(xf.value.t, p, 10 * px.value)
    xf.value = { ...xf.value, grab: { ...hit, from: p, t0: xf.value.t } }
    stage.value.setPointerCapture?.(event.pointerId)
    return
  }
  const tool = props.tool
  if (tool === 'zoom') {
    const box = stage.value.getBoundingClientRect()
    zoomAt(event.clientX - box.left, event.clientY - box.top, event.altKey ? 1 / 1.6 : 1.6)
    return
  }
  if (tool === 'move') {
    const layer = selectedLayer()
    if (!layer || layer.kind === 'adjustment') return
    const ids = movedIds(layer)
    if (!ids.length) return
    drag.value = { id: layer.id, ids: new Set(ids), box: boundsOf(ids), sx: p.x, sy: p.y, ox: layer.x, oy: layer.y, dx: 0, dy: 0 }
  } else if (tool === 'rect' || tool === 'objectSelect') {
    const q = snapPoint(p)
    temp.value = { kind: tool === 'rect' ? 'rect' : 'object', x0: q.x, y0: q.y, x1: q.x, y1: q.y }
  } else if (tool === 'patch') {
    if (patchSel.value && pointInPolygon(p.x, p.y, patchSel.value.points)) patchDrag.value = { from: p, dx: 0, dy: 0 }
    else {
      patchSel.value = null
      temp.value = { kind: 'patch', points: [[Math.round(p.x), Math.round(p.y)]] }
    }
  } else if (tool === 'lasso') {
    temp.value = { kind: 'lasso', points: [[Math.round(p.x), Math.round(p.y)]] }
  } else if (tool === 'wand') {
    magicWand(p)
    return
  } else if (tool === 'crop') {
    const q = snapPoint(p)
    cropDrag.value = { x0: q.x, y0: q.y }
    cropRect.value = null
  } else if (tool === 'eyedropper') {
    pickColour(p)
    return
  } else if (tool === 'text') {
    if (hitText(p)) {
      startText(p)
      return
    }
    textDrag.value = { x0: p.x, y0: p.y, x1: p.x, y1: p.y }
  } else if (tool === 'gradient') {
    gradLine.value = { from: p, to: p }
  } else if (tool === 'shape') {
    if (props.shape.kind === 'polygon') {
      clickPolygon(p, event.detail >= 2)
      return
    }
    shapeDrag.value = { kind: props.shape.kind, x0: p.x, y0: p.y, x1: p.x, y1: p.y, square: event.shiftKey }
  } else if (tool === 'pen') {
    clickPen(p)
    return
  } else if (tool === 'heal') {
    if (event.altKey) {
      healSource.value = p
      healOffset = null
      return
    }
    startHeal(p)
  } else if (RETOUCH[tool] || tool === 'quickSelect') {
    startPathTool(p, tool, event.altKey)
  } else if (PAINTING.includes(tool)) {
    if (tool === 'clone' && event.altKey && !maskTarget()) {
      cloneSource.value = p
      cloneOffset = null
      return
    }
    startStroke({ ...p, pressure: pressureOf(event) }, event.altKey)
  } else return
  stage.value.setPointerCapture?.(event.pointerId)
}

function onMove(event) {
  if (!props.doc) return
  if (panning.value) {
    pan.value = { x: panning.value.px + event.clientX - panning.value.sx, y: panning.value.py + event.clientY - panning.value.sy }
    return
  }
  const p = docPoint(event)
  hover.value = p
  altDown.value = event.altKey
  if (xf.value?.grab) {
    moveTransform(p, event.shiftKey)
  } else if (drag.value) {
    const moved = snapMove(drag.value.box, Math.round(p.x - drag.value.sx), Math.round(p.y - drag.value.sy))
    drag.value = { ...drag.value, ...moved }
    bump()
  } else if (temp.value?.kind === 'rect' || temp.value?.kind === 'object') {
    const q = snapPoint(p)
    temp.value = { ...temp.value, x1: q.x, y1: q.y }
  } else if (temp.value?.kind === 'lasso' || temp.value?.kind === 'patch') {
    const lastPt = temp.value.points.at(-1)
    if (Math.hypot(p.x - lastPt[0], p.y - lastPt[1]) >= 3 * px.value) temp.value = { kind: temp.value.kind, points: [...temp.value.points, [Math.round(p.x), Math.round(p.y)]] }
  } else if (patchDrag.value) {
    patchDrag.value = { ...patchDrag.value, dx: Math.round(p.x - patchDrag.value.from.x), dy: Math.round(p.y - patchDrag.value.from.y) }
  } else if (textDrag.value) {
    textDrag.value = { ...textDrag.value, x1: p.x, y1: p.y }
  } else if (cropDrag.value) {
    const q = snapPoint(p)
    cropRect.value = normRect({ x0: cropDrag.value.x0, y0: cropDrag.value.y0, x1: q.x, y1: q.y })
  } else if (gradLine.value) {
    gradLine.value = { ...gradLine.value, to: p }
  } else if (shapeDrag.value) {
    shapeDrag.value = { ...shapeDrag.value, x1: p.x, y1: p.y, square: event.shiftKey }
  } else if (healPath.value) {
    const lastPt = healPath.value.points.at(-1)
    if (Math.hypot(p.x - lastPt[0], p.y - lastPt[1]) >= Math.max(1, props.brush.size * 0.15)) healPath.value = { ...healPath.value, points: [...healPath.value.points, [p.x, p.y]] }
  } else if (stroke) {
    const q = { ...p, pressure: pressureOf(event) }
    if (stroke.ready) strokeTo(q)
    else stroke.pending.push(q)
  }
}

function onUp() {
  if (panning.value) {
    panning.value = null
    return
  }
  if (xf.value?.grab) {
    xf.value = { ...xf.value, grab: null }
    schedule()
  } else if (drag.value) {
    const d = drag.value
    drag.value = null
    if (d.dx || d.dy) finishMove(d)
    else bump()
  } else if (temp.value?.kind === 'rect') {
    const r = normRect(temp.value)
    temp.value = null
    setSelection(r.w >= 2 && r.h >= 2 ? { kind: 'rect', ...r } : null, r.w >= 2 && r.h >= 2 ? 'Rectangle selected' : 'Deselected', r.w >= 2 && r.h >= 2 ? { op: { op: 'select', ...r } } : undefined)
  } else if (temp.value?.kind === 'object') {
    const r = normRect(temp.value)
    temp.value = null
    if (r.w >= 4 && r.h >= 4) runObjectSelect(r)
  } else if (temp.value?.kind === 'patch') {
    const points = temp.value.points
    temp.value = null
    patchSel.value = points.length >= 3 ? { points } : null
  } else if (patchDrag.value) {
    const d = patchDrag.value
    patchDrag.value = null
    if (d.dx || d.dy) runPatch(d.dx, d.dy)
  } else if (textDrag.value) {
    const t = textDrag.value
    textDrag.value = null
    const w = Math.abs(t.x1 - t.x0)
    const h = Math.abs(t.y1 - t.y0)
    if (w >= 8 && h >= 8) startText({ x: Math.min(t.x0, t.x1), y: Math.min(t.y0, t.y1) }, { w: Math.round(w), h: Math.round(h) })
    else startText({ x: t.x0, y: t.y0 })
  } else if (temp.value?.kind === 'lasso') {
    const points = temp.value.points
    temp.value = null
    setSelection(points.length >= 3 ? { kind: 'lasso', points } : null, points.length >= 3 ? 'Lasso selected' : 'Deselected')
  } else if (cropDrag.value) {
    cropDrag.value = null
    if (cropRect.value && (cropRect.value.w < 2 || cropRect.value.h < 2)) cropRect.value = null
  } else if (gradLine.value) {
    const g = gradLine.value
    gradLine.value = null
    if (Math.hypot(g.to.x - g.from.x, g.to.y - g.from.y) >= 2) runGradient(g)
  } else if (shapeDrag.value) {
    const sd = shapeDrag.value
    shapeDrag.value = null
    const spec = shapeSpec(sd)
    if (spec) runShape(spec)
  } else if (healPath.value) {
    endPath()
  } else if (stroke) {
    endStroke()
  }
}

function onLeave() {
  hover.value = null
}

function onWheel(event) {
  if (!props.doc) return
  event.preventDefault()
  if (!event.ctrlKey) {
    pan.value = { x: pan.value.x - event.deltaX, y: pan.value.y - event.deltaY }
    return
  }
  const box = stage.value.getBoundingClientRect()
  zoomAt(event.clientX - box.left, event.clientY - box.top, Math.pow(1.0015, -event.deltaY))
}

// Zoom by factor keeping the point (mx, my) of the stage where it is.
function zoomAt(mx, my, factor) {
  const s0 = scale.value
  const next = Math.min(32 / fit.value, Math.max(0.1, zoom.value * factor))
  const s1 = fit.value * next
  const bx = (stageSize.value.w - W.value * s0) / 2 + pan.value.x
  const by = (stageSize.value.h - H.value * s0) / 2 + pan.value.y
  const docX = (mx - bx) / s0
  const docY = (my - by) / s0
  zoom.value = next
  pan.value = { x: mx - docX * s1 - (stageSize.value.w - W.value * s1) / 2, y: my - docY * s1 - (stageSize.value.h - H.value * s1) / 2 }
}

function zoomBy(factor) {
  zoomAt(stageSize.value.w / 2, stageSize.value.h / 2, factor)
}

function resetView() {
  zoom.value = 1
  pan.value = { x: 0, y: 0 }
}

function actualSize() {
  zoom.value = 1 / fit.value
  pan.value = { x: 0, y: 0 }
}

function pickColour(p) {
  const x = Math.floor(p.x * shownScale.value)
  const y = Math.floor(p.y * shownScale.value)
  const out = display.value
  if (x < 0 || y < 0 || x >= out.width || y >= out.height) return
  const [r, g, b] = out.getContext('2d', { willReadFrequently: true }).getImageData(x, y, 1, 1).data
  emit('colour', rgbToHex(r, g, b))
}

// ---------- moving a layer (its mask comes along) ----------

async function finishMove(d) {
  const doc = props.doc
  const head = doc.layers.find((l) => l.id === d.id)
  if (!head) return
  let next = doc
  await prepare(doc).catch(() => {})
  for (const id of [...d.ids, ...(isGroup(head) ? [head.id] : [])]) {
    const layer = doc.layers.find((l) => l.id === id)
    if (!layer) continue
    if (!isGroup(layer)) next = setLayer(next, id, { x: layer.x + d.dx, y: layer.y + d.dy })
    if (layer.mask) {
      try {
        const m = fullMask(layer.mask, doc.width, doc.height)
        if (m) next = setMask(next, id, maskPngFrom(shiftMask(m, doc.width, doc.height, d.dx, d.dy), doc.width, doc.height))
      } catch {
        // the mask stays where it was
      }
    }
  }
  const op = head.kind === 'image' ? { op: 'transform', layer: head.name, dx: d.dx, dy: d.dy } : null
  emit('change', next, isGroup(head) ? 'Group moved' : 'Moved', op ? { op } : undefined)
}

// ---------- painting (pixels, or a layer's mask) ----------

// Painting goes into the mask when its thumbnail is chosen and the layer has one.
function maskTarget() {
  const l = selectedLayer()
  return props.editMask && l?.mask ? l : null
}

// The brush's preset and pen settings a stroke keeps.
function brushFeel() {
  const b = props.brush
  return { build: !!b.build, flow: b.flow ?? 1, texture: b.texture || 0, pSize: !!b.pressureSize, pOpacity: !!b.pressureOpacity }
}

async function startStroke(p, alt) {
  const doc = props.doc
  if (props.quickMask && quick && (props.tool === 'brush' || props.tool === 'eraser')) {
    // the quick mask: white paints the selection in, the eraser (or Alt) takes it out
    const ctx = quick.canvas.getContext('2d', { willReadFrequently: true })
    stroke = {
      ready: true,
      pending: [],
      target: 'quick',
      ox: 0,
      oy: 0,
      canvas: quick.canvas,
      ctx,
      tool: 'brush',
      base: asPixels(ctx.getImageData(0, 0, doc.width, doc.height)),
      image: ctx.getImageData(0, 0, doc.width, doc.height),
      cover: new Float32Array(doc.width * doc.height),
      mask: null,
      rgba: [255, 255, 255, 255],
      erase: props.tool === 'eraser' || alt,
      size: Math.max(1, props.brush.size),
      hardness: props.brush.hardness,
      opacity: props.brush.opacity,
      ...brushFeel(),
      last: null,
      carry: 0,
      touched: false,
    }
    strokeTo(p)
    return
  }
  const onMask = maskTarget()
  if (props.tool === 'clone' && !onMask && !cloneSource.value) {
    emit('problem', 'Alt+click to choose where the clone stamp copies from.')
    return
  }
  stroke = { ready: false, pending: [p] }
  const mine = stroke
  try {
    let layer = selectedLayer()
    let base = doc
    if (onMask) {
      await prepare(doc)
      const sel = await selectionMask(doc)
      if (stroke !== mine) return
      const src = cache.get(layer.mask)
      const canvas = makeCanvas(doc.width, doc.height)
      const ctx = canvas.getContext('2d', { willReadFrequently: true })
      ctx.fillStyle = '#000'
      ctx.fillRect(0, 0, doc.width, doc.height)
      if (src) ctx.drawImage(src, 0, 0, doc.width, doc.height)
      liveMask = { id: layer.id, canvas }
      const white = !(props.tool === 'eraser' || alt)
      Object.assign(mine, {
        ready: true,
        target: 'mask',
        id: layer.id,
        doc,
        ox: 0,
        oy: 0,
        canvas,
        ctx,
        tool: 'brush',
        base: asPixels(ctx.getImageData(0, 0, doc.width, doc.height)),
        image: ctx.getImageData(0, 0, doc.width, doc.height),
        cover: new Float32Array(doc.width * doc.height),
        mask: sel,
        rgba: white ? [255, 255, 255, 255] : [0, 0, 0, 255],
        size: Math.max(1, props.brush.size),
        hardness: props.brush.hardness,
        opacity: props.brush.opacity,
        ...brushFeel(),
        last: null,
        carry: 0,
        touched: false,
      })
    } else {
      if (!layer || layer.kind !== 'image' || !layer.visible) {
        if (props.tool !== 'brush') {
          stroke = null
          emit('problem', layer && !layer.visible ? 'That layer is hidden - show it to paint on it.' : 'Choose a picture layer to work on.')
          return
        }
        // a brush on a text or adjustment layer paints on a new transparent layer instead
        const blank = makeCanvas(doc.width, doc.height).toDataURL('image/png')
        remember(blank, makeCanvas(doc.width, doc.height))
        const made = addFullLayer(doc, blank, 'Paint', props.selectedId)
        base = made.doc
        layer = base.layers.find((l) => l.id === made.id)
        emit('change', base, 'New layer')
        emit('select', layer.id)
      }
      await prepare(base)
      const sel = await selectionMask(doc)
      if (stroke !== mine) return
      const src = cache.get(layer.png)
      const canvas = makeCanvas(src.width, src.height)
      const ctx = canvas.getContext('2d', { willReadFrequently: true })
      ctx.drawImage(src, 0, 0)
      const w = canvas.width
      const h = canvas.height
      if (props.tool === 'clone' && !cloneOffset) cloneOffset = { x: cloneSource.value.x - p.x, y: cloneSource.value.y - p.y }
      live = { id: layer.id, canvas }
      Object.assign(mine, {
        ready: true,
        target: 'pixels',
        id: layer.id,
        doc: base,
        ox: layer.x,
        oy: layer.y,
        canvas,
        ctx,
        tool: props.tool,
        base: asPixels(ctx.getImageData(0, 0, w, h)),
        image: ctx.getImageData(0, 0, w, h),
        cover: new Float32Array(w * h),
        mask: sel ? { data: sel, width: doc.width, height: doc.height, ox: layer.x, oy: layer.y } : null,
        rgba: hexToRgba(props.primary),
        size: Math.max(1, props.brush.size),
        hardness: props.brush.hardness,
        opacity: props.brush.opacity,
        ...brushFeel(),
        points: [],
        last: null,
        carry: 0,
        touched: false,
      })
    }
    const queued = mine.pending
    mine.pending = []
    for (const q of queued) strokeTo(q)
    if (mine.ended) endStroke()
  } catch (reason) {
    stroke = null
    live = null
    liveMask = null
    emit('problem', String(reason?.message || reason))
  }
}

function dabAt(x, y, pressure = 1) {
  const s = stroke
  const lx = x - s.ox
  const ly = y - s.oy
  let box
  if (s.tool === 'clone') {
    box = cloneDab(asPixels(s.image), { x: lx + cloneOffset.x, y: ly + cloneOffset.y }, { x: lx, y: ly }, s.size, s.hardness, s.base, s.mask)
  } else {
    // presets (soft, hard, airbrush build-up, texture) and the pen's pressure on size and opacity
    const pr = pressured(s.size, pressure, { size: s.pSize, opacity: s.pOpacity })
    box = presetDab(s.cover, s.image.width, s.image.height, lx, ly, pr.size, s.hardness, s.mask, { strength: pr.strength, build: s.build, flow: s.flow, texture: s.texture })
    if (box.w && box.h) paintCover(asPixels(s.image), s.base, s.cover, box, { rgba: s.rgba, opacity: s.opacity, erase: s.erase ?? s.tool === 'eraser' })
  }
  if (!box.w || !box.h) return
  s.touched = true
  s.ctx.putImageData(s.image, 0, 0, box.x, box.y, box.w, box.h)
  if (s.target === 'quick') drawQuick()
  else bump()
}

// Dabs along the way, a little apart (the pressure eased between the points).
function strokeTo(p) {
  const s = stroke
  if (s.points && s.points.length < 2000) s.points.push([Math.round(p.x), Math.round(p.y)])
  const pr = p.pressure ?? 1
  if (!s.last) {
    dabAt(p.x, p.y, pr)
    s.last = p
    return
  }
  const step = Math.max(1, s.size * 0.15)
  const dist = Math.hypot(p.x - s.last.x, p.y - s.last.y)
  const p0 = s.last.pressure ?? 1
  let d = step - s.carry
  for (; d <= dist; d += step) dabAt(s.last.x + ((p.x - s.last.x) * d) / dist, s.last.y + ((p.y - s.last.y) * d) / dist, p0 + (pr - p0) * (d / dist))
  s.carry = dist - (d - step)
  s.last = p
}

function endStroke() {
  const s = stroke
  if (!s) return
  // still getting ready: it finishes itself once it is
  if (!s.ready) {
    s.ended = true
    return
  }
  stroke = null
  if (s.target === 'quick') return drawQuick()
  live = null
  liveMask = null
  if (!s.touched) return bump()
  const png = s.canvas.toDataURL('image/png')
  remember(png, s.canvas)
  const now = props.doc?.layers.some((l) => l.id === s.id) ? props.doc : s.doc
  version += 1
  if (s.target === 'mask') {
    emit('change', setMask(now, s.id, png), s.rgba[0] ? 'Mask painted' : 'Mask hidden')
    return
  }
  const label = { brush: 'Brush stroke', eraser: 'Erased', clone: 'Clone stamp' }[s.tool]
  const layer = now.layers.find((l) => l.id === s.id)
  // a hand brush stroke is kept as her brush op (for actions)
  const op = s.tool === 'brush' && s.points?.length ? { op: 'brush', layer: layer?.name, points: s.points, size: Math.round(s.size), hardness: s.hardness, opacity: s.opacity, color: props.primary } : null
  emit('change', setLayer(now, s.id, { png }), label, op ? { op } : undefined)
}

// ---------- the quick mask ----------

// Q on: the selection (or nothing) becomes a mask he paints; red shows what is not selected.
async function startQuickMask() {
  const doc = props.doc
  if (!doc) return
  const sel = await selectionMask(doc)
  const c = makeCanvas(doc.width, doc.height)
  if (sel) {
    const ctx = c.getContext('2d')
    const img = ctx.createImageData(doc.width, doc.height)
    for (let j = 0; j < sel.length; j++) {
      const i = j * 4
      img.data[i] = img.data[i + 1] = img.data[i + 2] = 255
      img.data[i + 3] = sel[j]
    }
    ctx.putImageData(img, 0, 0)
  }
  quick = { canvas: c }
  await nextTick()
  drawQuick()
}

// Q off: the painted mask -> the selection (one history step). Nothing painted in: no selection.
function endQuickMask() {
  const doc = props.doc
  const q = quick
  quick = null
  if (!doc || !q) return
  const d = q.canvas.getContext('2d', { willReadFrequently: true }).getImageData(0, 0, doc.width, doc.height).data
  const mask = new Uint8Array(doc.width * doc.height)
  let any = false
  for (let j = 0; j < mask.length; j++) {
    mask[j] = d[j * 4 + 3]
    if (mask[j]) any = true
  }
  setSelection(any ? { kind: 'mask', png: selectionPng(mask, doc.width, doc.height) } : null, any ? 'Quick mask' : 'Deselected')
}

let quickFrame = 0
function drawQuick() {
  if (quickFrame) return
  quickFrame = requestAnimationFrame(() => {
    quickFrame = 0
    const out = quickView.value
    if (!out || !quick) return
    if (out.width !== quick.canvas.width || out.height !== quick.canvas.height) {
      out.width = quick.canvas.width
      out.height = quick.canvas.height
    }
    const ctx = out.getContext('2d')
    ctx.globalCompositeOperation = 'source-over'
    ctx.fillStyle = 'rgb(255, 40, 60)'
    ctx.fillRect(0, 0, out.width, out.height)
    ctx.globalCompositeOperation = 'destination-out'
    ctx.drawImage(quick.canvas, 0, 0)
    ctx.globalCompositeOperation = 'source-over'
  })
}

// ---------- the path brushes: retouch (engine3), spot heal, quick select ----------

function startPathTool(p, tool, alt) {
  if (tool !== 'quickSelect') {
    const layer = selectedLayer()
    if (!layer || layer.kind !== 'image' || !layer.visible) {
      emit('problem', 'Choose a visible picture layer to work on.')
      return
    }
  }
  healPath.value = { points: [[p.x, p.y]], tool, alt }
}

function endPath() {
  const path = healPath.value
  if (!path) return
  if (path.tool === 'quickSelect') return runQuickSelect(path)
  if (RETOUCH[path.tool]) return runRetouch(path)
  return endHeal()
}

const roundPts = (pts) => pts.map(([x, y]) => [Math.round(x), Math.round(y)])

async function runRetouch(path) {
  healPath.value = null
  const doc = props.doc
  const layer = selectedLayer()
  if (!layer || layer.kind !== 'image') return
  working.value = true
  try {
    const pixels = await pixelsOf(doc, layer.id)
    const sel = await selectionMask(doc)
    const local = sel ? localMask(sel, doc.width, doc.height, layer.x, layer.y, pixels.width, pixels.height) : null
    const size = Math.max(2, Math.round(props.brush.size))
    const strength = props.brush.strength ?? 0.5
    const range = props.brush.range || 'midtones'
    const mode = props.brush.mode === 'desaturate' ? 'desaturate' : 'saturate'
    const points = path.points.map(([x, y]) => [x - layer.x, y - layer.y])
    const out = RETOUCH[path.tool](pixels, { points, size, hardness: props.brush.hardness, strength, range, mode }, local)
    const op = { op: RETOUCH_OPS[path.tool], layer: layer.name, points: roundPts(path.points).slice(0, 1000), size }
    if (path.tool !== 'spotHeal') op.strength = strength
    if (path.tool === 'dodge' || path.tool === 'burn') op.range = range
    if (path.tool === 'sponge') op.mode = mode
    emit('change', setLayer(props.doc, layer.id, { png: pngFrom(out?.data ? out : out?.pixels || pixels) }), RETOUCH_LABELS[path.tool], { op })
  } catch (reason) {
    emit('problem', String(reason?.message || reason))
  } finally {
    working.value = false
  }
}

async function runQuickSelect(path) {
  healPath.value = null
  const doc = props.doc
  working.value = true
  try {
    const comp = await composite(doc)
    const prev = await selectionMask(doc)
    const size = Math.max(2, Math.round(props.brush.size))
    const mask = quickSelect(comp, [{ points: path.points, size }], { add: !path.alt, prevMask: prev, size })
    const any = mask.some((v) => v > 0)
    const strokes = [{ points: roundPts(path.points).slice(0, 500), size, mode: path.alt ? 'subtract' : 'add' }]
    setSelection(any ? { kind: 'mask', png: selectionPng(mask, doc.width, doc.height) } : null, any ? 'Quick selected' : 'Deselected', { op: { op: 'select', quick: { strokes } } })
  } catch (reason) {
    emit('problem', String(reason?.message || reason))
  } finally {
    working.value = false
  }
}

async function runObjectSelect(r) {
  const doc = props.doc
  working.value = true
  try {
    const comp = await composite(doc)
    const mask = objectSelect(comp, r)
    if (!mask.some((v) => v > 0)) return emit('problem', 'No object was found in that box.')
    setSelection({ kind: 'mask', png: selectionPng(mask, doc.width, doc.height) }, 'Object selected', { op: { op: 'select', object: { ...r } } })
  } catch (reason) {
    emit('problem', String(reason?.message || reason))
  } finally {
    working.value = false
  }
}

// The patch: the area drawn is filled from where it was dragged to.
async function runPatch(dx, dy) {
  const doc = props.doc
  const sel = patchSel.value
  const layer = selectedLayer()
  patchSel.value = null
  if (!sel || !layer || layer.kind !== 'image') return emit('problem', 'Choose a picture layer to patch.')
  working.value = true
  try {
    const pixels = await pixelsOf(doc, layer.id)
    const area = maskFromLasso(doc.width, doc.height, sel.points)
    const hole = localMask(area, doc.width, doc.height, layer.x, layer.y, pixels.width, pixels.height)
    const out = patch(pixels, hole, { dx, dy })
    const b = maskBounds(area, doc.width, doc.height)
    const op = b ? { op: 'patch', layer: layer.name, ...b, dx, dy } : null
    emit('change', setLayer(props.doc, layer.id, { png: pngFrom(out) }), 'Patched', op ? { op } : undefined)
  } catch (reason) {
    emit('problem', String(reason?.message || reason))
  } finally {
    working.value = false
  }
}

// ---------- healing brush ----------

function startHeal(p) {
  const layer = selectedLayer()
  if (!layer || layer.kind !== 'image' || !layer.visible) {
    emit('problem', 'Choose a visible picture layer to heal.')
    return
  }
  if (!healSource.value) {
    emit('problem', 'Alt+click to choose where the healing brush copies from.')
    return
  }
  healPath.value = { points: [[p.x, p.y]], tool: 'heal' }
}

async function endHeal() {
  const path = healPath.value
  healPath.value = null
  const doc = props.doc
  const layer = selectedLayer()
  if (!path || !layer || layer.kind !== 'image') return
  working.value = true
  try {
    const first = path.points[0]
    if (!healOffset) healOffset = { x: healSource.value.x - first[0], y: healSource.value.y - first[1] }
    const pixels = await pixelsOf(doc, layer.id)
    const sel = await selectionMask(doc)
    const local = sel ? localMask(sel, doc.width, doc.height, layer.x, layer.y, pixels.width, pixels.height) : null
    const points = path.points.map(([x, y]) => [x - layer.x, y - layer.y])
    const out = heal(pixels, { source: [first[0] + healOffset.x - layer.x, first[1] + healOffset.y - layer.y], points, size: Math.max(2, props.brush.size), hardness: props.brush.hardness }, local)
    const fresh = props.doc
    emit('change', setLayer(fresh, layer.id, { png: pngFrom(out) }), 'Healed spot')
  } catch (reason) {
    emit('problem', String(reason?.message || reason))
  } finally {
    working.value = false
  }
}

// ---------- gradient ----------

async function runGradient(g) {
  const doc = props.doc
  working.value = true
  try {
    let layer = selectedLayer()
    let base = doc
    let label = 'Gradient'
    if (!layer || layer.kind !== 'image') {
      const blank = makeCanvas(doc.width, doc.height)
      const png = blank.toDataURL('image/png')
      remember(png, blank)
      const made = addFullLayer(doc, png, 'Gradient', props.selectedId)
      base = made.doc
      layer = base.layers.find((l) => l.id === made.id)
      label = 'Gradient layer'
    }
    const pixels = await pixelsOf(base, layer.id)
    const sel = await selectionMask(doc)
    const local = sel ? localMask(sel, doc.width, doc.height, layer.x, layer.y, pixels.width, pixels.height) : null
    const spec = {
      kind: props.gradient?.kind === 'radial' ? 'radial' : 'linear',
      from: [g.from.x - layer.x, g.from.y - layer.y],
      to: [g.to.x - layer.x, g.to.y - layer.y],
      stops: props.gradient?.stops?.length ? props.gradient.stops : [{ at: 0, color: props.primary }, { at: 1, color: props.secondary }],
    }
    const out = drawGradient(pixels, spec, local)
    emit('change', setLayer(base, layer.id, { png: pngFrom(out) }), label)
    if (base !== doc) emit('select', layer.id)
  } catch (reason) {
    emit('problem', String(reason?.message || reason))
  } finally {
    working.value = false
  }
}

// ---------- shapes and the pen ----------

function shapeColours() {
  const s = props.shape || {}
  let fill = s.fillOn ? s.fill || props.primary : null
  const stroke = s.strokeOn ? s.stroke || props.secondary : null
  if (!fill && !stroke) fill = props.primary
  return { fill, stroke, width: Math.max(0, Number(s.width) || 0) }
}

function shapeSpec(sd) {
  let w = sd.x1 - sd.x0
  let h = sd.y1 - sd.y0
  if (Math.abs(w) < 2 && Math.abs(h) < 2) return null
  if (sd.kind === 'line') return { kind: 'line', points: [[sd.x0, sd.y0], [sd.x1, sd.y1]], ...shapeColours(), fill: null, stroke: shapeColours().stroke || props.shape?.stroke || props.primary, width: Math.max(1, Number(props.shape?.width) || 4) }
  if (sd.square) {
    const m = Math.max(Math.abs(w), Math.abs(h))
    w = Math.sign(w || 1) * m
    h = Math.sign(h || 1) * m
  }
  return { kind: sd.kind === 'ellipse' ? 'ellipse' : 'rect', x: Math.min(sd.x0, sd.x0 + w), y: Math.min(sd.y0, sd.y0 + h), w: Math.abs(w), h: Math.abs(h), ...shapeColours() }
}

const SHAPE_NAMES = { rect: 'Rectangle', ellipse: 'Ellipse', line: 'Line', polygon: 'Polygon' }

async function runShape(spec, doc = props.doc) {
  working.value = true
  try {
    const pixels = drawShape(makePixels(doc.width, doc.height), spec)
    const made = addFullLayer(doc, pngFrom(pixels), SHAPE_NAMES[spec.kind] || 'Shape', props.selectedId)
    emit('change', made.doc, `Shape: ${(SHAPE_NAMES[spec.kind] || 'shape').toLowerCase()}`)
    emit('select', made.id)
  } catch (reason) {
    emit('problem', String(reason?.message || reason))
  } finally {
    working.value = false
  }
}

function clickPolygon(p, done) {
  const pts = poly.value?.points || []
  const first = pts[0]
  if (done || (first && pts.length >= 3 && Math.hypot(p.x - first[0], p.y - first[1]) <= 8 * px.value)) {
    finishPolygon()
    return
  }
  poly.value = { points: [...pts, [p.x, p.y]] }
}

function finishPolygon() {
  const pts = poly.value?.points || []
  poly.value = null
  if (pts.length >= 3) runShape({ kind: 'polygon', points: pts, ...shapeColours() })
}

function clickPen(p) {
  const pts = pen.value?.points || []
  const first = pts[0]
  if (first && pts.length >= 3 && Math.hypot(p.x - first[0], p.y - first[1]) <= 8 * px.value) {
    penToSelection()
    return
  }
  pen.value = { points: [...pts, [p.x, p.y]] }
}

// The pen's path goes into PATHS (doc.paths) whatever it becomes.
function penToSelection() {
  const pts = pen.value?.points || []
  pen.value = null
  if (pts.length < 3) return emit('problem', 'A path needs three points or more.')
  const kept = addPath(props.doc, pts, true).doc
  emit('change', { ...kept, selection: { kind: 'lasso', points: pts.map(([x, y]) => [Math.round(x), Math.round(y)]) } }, 'Pen path selected')
}

function penToShape() {
  const pts = pen.value?.points || []
  pen.value = null
  if (pts.length < 2) return
  const kept = addPath(props.doc, pts, pts.length >= 3).doc
  if (pts.length >= 3) runShape({ kind: 'polygon', points: pts, ...shapeColours() }, kept)
  else runShape({ kind: 'line', points: pts, fill: null, stroke: shapeColours().stroke || props.primary, width: Math.max(1, Number(props.shape?.width) || 4) }, kept)
}

function keepPath() {
  const pts = pen.value?.points || []
  pen.value = null
  if (pts.length >= 2) emit('change', addPath(props.doc, pts, false).doc, 'Path kept')
}

const pathOf = (pts, closed) => (pts?.length ? `M${pts.map((q) => `${q[0]} ${q[1]}`).join('L')}${closed ? 'Z' : ''}` : '')
const penPath = computed(() => {
  const pts = pen.value?.points
  if (!pts?.length) return ''
  return pathOf(hover.value ? [...pts, [hover.value.x, hover.value.y]] : pts, false)
})
const polyPath = computed(() => {
  const pts = poly.value?.points
  if (!pts?.length) return ''
  return pathOf(hover.value ? [...pts, [hover.value.x, hover.value.y]] : pts, false)
})
const activePathD = computed(() => (props.activePath?.points?.length ? pathOf(props.activePath.points, props.activePath.closed) : ''))
const patchPath = computed(() => {
  const pts = temp.value?.kind === 'patch' ? temp.value.points : patchSel.value?.points
  return pts?.length > 1 ? pathOf(pts, temp.value?.kind !== 'patch') : ''
})
const patchMoved = computed(() => {
  const d = patchDrag.value
  if (!d || !patchSel.value) return ''
  return pathOf(patchSel.value.points.map(([x, y]) => [x + d.dx, y + d.dy]), true)
})
const textBoxDrag = computed(() => {
  const t = textDrag.value
  if (!t) return null
  return { x: Math.min(t.x0, t.x1), y: Math.min(t.y0, t.y1), w: Math.abs(t.x1 - t.x0), h: Math.abs(t.y1 - t.y0) }
})
const shapePreview = computed(() => {
  const sd = shapeDrag.value
  if (!sd) return null
  return shapeSpec(sd)
})

// ---------- free transform ----------

function emitTransform() {
  emit('transform', xf.value ? { ...xf.value.t } : null)
}

async function startFreeTransform() {
  const layer = selectedLayer()
  if (!props.doc || !layer) return
  if (layer.kind !== 'image') {
    emit('problem', isGroup(layer) ? 'Free transform works on one picture layer - choose one inside the group.' : 'Free transform works on picture layers - choose one first.')
    return
  }
  await prepare(props.doc)
  const src = cache.get(layer.png)
  if (!src) return
  xf.value = { id: layer.id, ox: layer.x, oy: layer.y, t: startTransform(layer.x, layer.y, src.width, src.height), grab: null }
  emitTransform()
  bump()
}

function moveTransform(p, shift) {
  const x = xf.value
  const g = x.grab
  let t
  if (g.kind === 'scale') t = scaleFromCorner(g.t0, g.corner, p, shift)
  else if (g.kind === 'move') {
    t = { ...g.t0, cx: g.t0.cx + p.x - g.from.x, cy: g.t0.cy + p.y - g.from.y }
    const fix = snapMove(transformBounds(t), 0, 0)
    t = { ...t, cx: t.cx + fix.dx, cy: t.cy + fix.dy }
  } else t = rotateAround(g.t0, g.t0.rotate, g.from, p, shift ? 15 : 0)
  xf.value = { ...x, t }
  emitTransform()
  bump()
}

function setTransform(t) {
  if (!xf.value || !t) return
  xf.value = { ...xf.value, t: { ...xf.value.t, ...t } }
  emitTransform()
  bump()
}

function cancelTransform() {
  if (!xf.value) return
  xf.value = null
  emitTransform()
  bump()
}

async function commitTransform() {
  const x = xf.value
  if (!x) return
  const doc = props.doc
  const layer = doc.layers.find((l) => l.id === x.id)
  xf.value = null
  emitTransform()
  if (!layer) return bump()
  working.value = true
  try {
    const t = x.t
    const m = matrixFor({ scale: [t.sx, t.sy], rotate: (t.rotate * 180) / Math.PI, cx: t.w / 2, cy: t.h / 2, dx: t.cx - t.w / 2, dy: t.cy - t.h / 2 })
    const pixels = await pixelsOf(doc, layer.id)
    const out = transformPixels(pixels, m, doc.width, doc.height)
    let next = setLayer(doc, layer.id, { png: pngFrom(out), x: 0, y: 0 })
    if (layer.mask) {
      const mask = fullMask(layer.mask, doc.width, doc.height)
      if (mask) {
        // the mask is document sized: back into the layer's frame, then the same transform
        const mm = { ...m, e: m.e - m.a * x.ox - m.c * x.oy, f: m.f - m.b * x.ox - m.d * x.oy }
        const moved = transformPixels(maskAsPixels(mask, doc.width, doc.height), mm, doc.width, doc.height)
        const values = new Uint8Array(doc.width * doc.height)
        for (let j = 0; j < values.length; j++) values[j] = (moved.data[j * 4] * moved.data[j * 4 + 3]) / 255
        next = setMask(next, layer.id, maskPngFrom(values, doc.width, doc.height))
      }
    }
    version += 1
    emit('change', next, 'Free transform')
  } catch (reason) {
    emit('problem', String(reason?.message || reason))
    bump()
  } finally {
    working.value = false
  }
}

// ---------- text ----------

function hitText(p) {
  const layers = props.doc.layers
  for (let i = layers.length - 1; i >= 0; i--) {
    const l = layers[i]
    if (l.kind !== 'text' || !l.visible) continue
    const { w, h } = measure(l)
    if (p.x >= l.x && p.y >= l.y && p.x <= l.x + w && p.y <= l.y + h) return l
  }
  return null
}

async function startText(p, box = null) {
  const hit = box ? null : hitText(p)
  if (hit) {
    editing.value = { id: hit.id, x: hit.x, y: hit.y, value: hit.text.value, text: { ...hit.text }, box: hit.textBox || null }
    emit('select', hit.id)
    bump()
  } else {
    const text = { ...defaultText(H.value, props.primary) }
    if (box) text.size = Math.max(12, Math.round(Math.min(text.size, box.h / 3)))
    editing.value = { id: null, x: Math.round(p.x), y: Math.round(p.y), value: '', text, box: box ? { w: box.w, h: box.h, align: 'left', lineHeight: 1.2, tracking: 0 } : null }
  }
  await nextTick()
  textBox.value?.focus()
  textBox.value?.select()
}

function commitText() {
  const e = editing.value
  if (!e) return
  editing.value = null
  const value = e.value.replace(/\s+$/, '')
  if (e.id) {
    const layer = props.doc.layers.find((l) => l.id === e.id)
    if (layer && value && value !== layer.text.value) emit('change', setLayer(props.doc, e.id, { text: { ...layer.text, value } }), 'Text edited')
    else bump()
    return
  }
  if (!value) return
  let next = addTextLayer(props.doc, value, e.x, e.y, e.text)
  const made = next.layers.at(-1)
  if (e.box) next = setLayer(next, made.id, { textBox: e.box })
  const op = { op: 'text', value, x: e.x, y: e.y, size: e.text.size, color: e.text.color, font: e.text.font, weight: e.text.weight }
  if (e.box) op.box = { ...e.box }
  emit('change', next, e.box ? 'Paragraph text added' : 'Text added', { op })
  emit('select', made.id)
}

function cancelText() {
  editing.value = null
  bump()
}

function onTextKey(event) {
  if (event.key === 'Enter' && !event.shiftKey && !editing.value?.box) {
    event.preventDefault()
    commitText()
  } else if (event.key === 'Enter' && event.ctrlKey) {
    event.preventDefault()
    commitText()
  } else if (event.key === 'Escape') {
    event.preventDefault()
    cancelText()
  }
  event.stopPropagation()
}

const textStyle = computed(() => {
  const e = editing.value
  if (!e) return {}
  const base = {
    left: `${e.x}px`,
    top: `${e.y}px`,
    font: textFont(e.text),
    lineHeight: e.box?.lineHeight ?? 1.2,
    color: e.text.color,
  }
  if (!e.box) return { ...base, minWidth: `${Math.max(40, e.text.size * 3)}px` }
  return { ...base, width: `${e.box.w}px`, minHeight: `${e.box.h}px`, whiteSpace: 'pre-wrap', textAlign: e.box.align, letterSpacing: `${e.box.tracking || 0}px`, fieldSizing: 'fixed', overflowWrap: 'anywhere' }
})

// ---------- whole-document transforms (pixels of every image layer and every mask too) ----------

function rotateDraw(t) {
  return (src) => {
    const c = t === 2 ? makeCanvas(src.width, src.height) : makeCanvas(src.height, src.width)
    const ctx = c.getContext('2d')
    if (t === 1) ctx.translate(src.height, 0)
    else if (t === 2) ctx.translate(src.width, src.height)
    else ctx.translate(0, src.width)
    ctx.rotate((t === 3 ? -Math.PI : Math.PI * t) / 2)
    ctx.drawImage(src, 0, 0)
    return { canvas: c }
  }
}

function flipDraw(axis) {
  return (src) => {
    const c = makeCanvas(src.width, src.height)
    const ctx = c.getContext('2d')
    if (axis === 'v') {
      ctx.translate(0, src.height)
      ctx.scale(1, -1)
    } else {
      ctx.translate(src.width, 0)
      ctx.scale(-1, 1)
    }
    ctx.drawImage(src, 0, 0)
    return { canvas: c }
  }
}

async function transformDoc(doc, op) {
  await prepare(doc)
  const sizes = Object.fromEntries(doc.layers.map((l) => [l.id, measure(l)]))
  let next
  let draw
  let drawMask // document-sized mask canvas -> canvas of the new document's size
  if (op.op === 'crop') {
    const r = { x: Math.round(op.x), y: Math.round(op.y), w: Math.max(1, Math.round(op.w)), h: Math.max(1, Math.round(op.h)) }
    next = cropDoc(doc, r)
    draw = (src, l) => {
      // keep only the part inside the crop
      const x0 = Math.max(l.x, r.x)
      const y0 = Math.max(l.y, r.y)
      const x1 = Math.min(l.x + src.width, r.x + r.w)
      const y1 = Math.min(l.y + src.height, r.y + r.h)
      if (x1 <= x0 || y1 <= y0) return { canvas: makeCanvas(1, 1), x: 0, y: 0 }
      const c = makeCanvas(x1 - x0, y1 - y0)
      c.getContext('2d').drawImage(src, l.x - x0, l.y - y0)
      return { canvas: c, x: x0 - r.x, y: y0 - r.y }
    }
    drawMask = (src) => {
      const c = makeCanvas(r.w, r.h)
      const ctx = c.getContext('2d')
      ctx.fillStyle = '#000'
      ctx.fillRect(0, 0, r.w, r.h)
      ctx.drawImage(src, -r.x, -r.y, doc.width, doc.height)
      return c
    }
  } else if (op.op === 'rotate') {
    const t = ((op.turns % 4) + 4) % 4
    next = rotateDoc(doc, t, sizes)
    draw = rotateDraw(t)
    drawMask = (src) => rotateDraw(t)(src).canvas
  } else if (op.op === 'flip') {
    next = flipDoc(doc, op.axis, sizes)
    draw = flipDraw(op.axis)
    drawMask = (src) => flipDraw(op.axis)(src).canvas
  } else if (op.op === 'resize') {
    next = resizeDoc(doc, op.width, op.height)
    const sx = next.width / doc.width
    const sy = next.height / doc.height
    draw = (src) => {
      const c = makeCanvas(src.width * sx, src.height * sy)
      const ctx = c.getContext('2d')
      ctx.imageSmoothingQuality = 'high'
      ctx.drawImage(src, 0, 0, c.width, c.height)
      return { canvas: c }
    }
    drawMask = (src) => {
      const c = makeCanvas(next.width, next.height)
      const ctx = c.getContext('2d')
      ctx.imageSmoothingQuality = 'high'
      ctx.drawImage(src, 0, 0, c.width, c.height)
      return c
    }
  } else if (op.op === 'canvas') {
    const width = Math.max(1, Math.round(op.width))
    const height = Math.max(1, Math.round(op.height))
    const { dx, dy } = Number.isFinite(op.x) && Number.isFinite(op.y) ? { dx: Math.round(op.x), dy: Math.round(op.y) } : canvasOffset(doc.width, doc.height, width, height, op.anchor)
    next = { ...doc, width, height, layers: doc.layers.map((l) => ({ ...l, x: l.x + dx, y: l.y + dy })), selection: null }
    draw = null
    drawMask = (src) => {
      const c = makeCanvas(width, height)
      const ctx = c.getContext('2d')
      ctx.fillStyle = '#000'
      ctx.fillRect(0, 0, width, height)
      ctx.drawImage(src, dx, dy, doc.width, doc.height)
      return c
    }
  } else {
    throw new Error(`Unknown transform "${op.op}"`)
  }
  const layers = next.layers.map((l, i) => {
    const old = doc.layers[i]
    let out = l
    if (draw && old.kind === 'image' && cache.get(old.png)) {
      const done = draw(cache.get(old.png), old)
      const png = done.canvas.toDataURL('image/png')
      remember(png, done.canvas)
      out = { ...out, png, ...(done.x !== undefined ? { x: done.x, y: done.y } : {}) }
    }
    if (old.mask && cache.get(old.mask)) {
      // masks are document sized: drawn whole onto the new document
      const src = cache.get(old.mask)
      const full = src.width === doc.width && src.height === doc.height ? src : (() => {
        const c = makeCanvas(doc.width, doc.height)
        c.getContext('2d').drawImage(src, 0, 0, doc.width, doc.height)
        return c
      })()
      const c = drawMask(full)
      const png = c.toDataURL('image/png')
      remember(png, c)
      out = { ...out, mask: png }
    }
    return out
  })
  return { ...next, layers }
}

const TRANSFORM_LABELS = {
  rotate: (o) => (o.turns === 1 ? 'Rotated right' : o.turns === 2 ? 'Rotated 180°' : 'Rotated left'),
  flip: (o) => (o.axis === 'h' ? 'Flipped horizontally' : 'Flipped vertically'),
  crop: () => 'Cropped',
  resize: (o) => `Image size ${o.width}×${o.height}`,
  canvas: (o) => `Canvas size ${o.width}×${o.height}`,
}

async function runTransform(op) {
  if (!props.doc) return
  working.value = true
  try {
    emit('change', await transformDoc(props.doc, op), TRANSFORM_LABELS[op.op]?.(op) ?? 'Transformed')
  } catch (reason) {
    emit('problem', String(reason?.message || reason))
  } finally {
    working.value = false
  }
}

const rotate = (turns) => runTransform({ op: 'rotate', turns })
const flip = (axis) => runTransform({ op: 'flip', axis })

// Enter: the crop rect, or the selection's bounds when nothing was dragged.
async function applyCrop() {
  let r = cropRect.value
  if (!r && props.doc?.selection) {
    const sel = props.doc.selection
    if (sel.kind === 'rect') r = { x: sel.x, y: sel.y, w: sel.w, h: sel.h }
    else {
      const mask = await selectionMask(props.doc)
      r = mask ? maskBounds(mask, props.doc.width, props.doc.height) : null
    }
  }
  cropRect.value = null
  if (!r || r.w < 1 || r.h < 1) {
    emit('problem', 'Drag the area to keep with the crop tool, or select it first.')
    return
  }
  await runTransform({ op: 'crop', ...r })
}

function cancelCrop() {
  cropRect.value = null
  cropDrag.value = null
}

const cropShade = computed(() => {
  const r = cropRect.value
  return r ? `M0 0H${W.value}V${H.value}H0Z${rectPath(r)}` : ''
})

// ---------- flattening ----------

async function renderDoc(doc, withAdjust, matte) {
  let pixels = await composite(doc)
  if (withAdjust && !isAdjustEmpty(doc.adjust)) pixels = applyAdjust(pixels, doc.adjust)
  const flat = canvasOf(pixels)
  if (!matte) return flat
  const c = makeCanvas(doc.width, doc.height)
  const ctx = c.getContext('2d')
  ctx.fillStyle = matte
  ctx.fillRect(0, 0, c.width, c.height)
  ctx.drawImage(flat, 0, 0)
  return c
}

// base64 without the data: prefix; format 'jpeg' is encoded here on white (the core has no JPEG encoder)
async function flattenDoc(doc, { withAdjust = true, format = 'png', quality = 92 } = {}) {
  if (!doc) return null
  const jpeg = format === 'jpeg'
  const c = await renderDoc(doc, withAdjust, jpeg ? '#ffffff' : '')
  // png, jpeg or webp (a project's picture saves back in its own format, Phase 235)
  const type = jpeg ? 'image/jpeg' : format === 'webp' ? 'image/webp' : 'image/png'
  return c.toDataURL(type, jpeg || format === 'webp' ? quality / 100 : undefined).split(',')[1]
}

function flattenBase64(withAdjust = true, options = {}) {
  return flattenDoc(props.doc, { ...options, withAdjust })
}

async function layerPngBase64(id) {
  const layer = props.doc?.layers.find((l) => l.id === id)
  if (!layer?.png) return null
  const prefix = 'data:image/png;base64,'
  if (layer.png.startsWith(prefix)) return layer.png.slice(prefix.length)
  await prepare(props.doc)
  return cache.get(layer.png)?.toDataURL('image/png').split(',')[1] ?? null
}

// A filter dialog's live look on one layer: fn(pixels at the render size, scale, selection mask).
function setPreview(spec) {
  preview = spec && spec.id && typeof spec.fn === 'function' ? spec : null
  bump()
}

// The picture small, as the canvas shows it now (NAVIGATOR, CHANNELS): { url, pixels }.
function thumbnail(maxSide = 220) {
  const out = display.value
  if (!props.doc || !out || !out.width) return null
  const k = Math.min(1, maxSide / Math.max(out.width, out.height))
  const c = makeCanvas(out.width * k, out.height * k)
  const ctx = c.getContext('2d', { willReadFrequently: true })
  ctx.imageSmoothingQuality = 'high'
  ctx.drawImage(out, 0, 0, c.width, c.height)
  return { url: c.toDataURL('image/png'), pixels: readCanvas(c) }
}

// The document point (x, y) to the middle of the stage.
function centerOn(x, y) {
  pan.value = { x: (W.value * scale.value) / 2 - x * scale.value, y: (H.value * scale.value) / 2 - y * scale.value }
}

watch(
  () => [stageSize.value.w, stageSize.value.h, scale.value, boardPos.value.x, boardPos.value.y],
  () => emit('view', { stageW: stageSize.value.w, stageH: stageSize.value.h, scale: scale.value, x: boardPos.value.x, y: boardPos.value.y }),
  { immediate: true },
)

// Her hand: glides over the canvas along points in document pixels.
function play(points, kind) {
  return companionCursor.value?.play ? companionCursor.value.play(points, kind) : Promise.resolve()
}

defineExpose({
  flattenBase64,
  flattenDoc,
  layerPngBase64,
  transformDoc,
  runTransform,
  rotate,
  flip,
  applyCrop,
  cancelCrop,
  resetView,
  actualSize,
  zoomBy,
  pixelsOf,
  maskOf,
  pngFrom,
  maskPngFrom,
  selectionPng,
  composite,
  selectionMask,
  selectionMaskNow,
  histogramBelow,
  setPreview,
  startFreeTransform,
  commitTransform,
  cancelTransform,
  setTransform,
  play,
  startQuickMask,
  endQuickMask,
  thumbnail,
  centerOn,
  keepPath,
})

// ---------- keys, size ----------

const typing = (el) => el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT' || el.isContentEditable)

function onKey(event) {
  if (!props.active || !props.doc) return
  if (event.key === 'Alt') altDown.value = true
  if (typing(document.activeElement)) return
  if (event.code === 'Space') {
    spaceDown.value = true
    event.preventDefault()
  } else if (event.key === 'Enter') {
    if (xf.value) commitTransform()
    else if (pen.value) event.ctrlKey ? keepPath() : event.shiftKey ? penToShape() : penToSelection()
    else if (poly.value) finishPolygon()
    else if (props.tool === 'crop') applyCrop()
    else return
    event.preventDefault()
  } else if (event.key === 'Escape') {
    if (xf.value) cancelTransform()
    if (cropRect.value) cancelCrop()
    pen.value = null
    poly.value = null
    temp.value = null
    gradLine.value = null
    shapeDrag.value = null
    patchSel.value = null
    patchDrag.value = null
    textDrag.value = null
  }
}
function onKeyUp(event) {
  if (event.code === 'Space') spaceDown.value = false
  if (event.key === 'Alt') altDown.value = false
}

watch(
  () => props.tool,
  (tool) => {
    if (tool !== 'crop') cancelCrop()
    if (tool !== 'pen') pen.value = null
    if (tool !== 'shape') poly.value = null
    if (tool !== 'patch') patchSel.value = null
    if (editing.value && tool !== 'text') commitText()
  },
)
watch(
  () => props.active,
  (on) => {
    if (!on) {
      spaceDown.value = false
      altDown.value = false
    }
  },
)

let observer
onMounted(() => {
  window.addEventListener('keydown', onKey)
  window.addEventListener('keyup', onKeyUp)
  observer = new ResizeObserver(([entry]) => {
    const r = entry.contentRect
    if (r.width && r.height) stageSize.value = { w: r.width, h: r.height }
  })
  if (stage.value) observer.observe(stage.value)
  schedule()
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKey)
  window.removeEventListener('keyup', onKeyUp)
  observer?.disconnect()
  cancelAnimationFrame(frame)
  cancelAnimationFrame(quickFrame)
  clearTimeout(restTimer)
})

const xfCursor = computed(() => {
  if (!xf.value || !hover.value) return ''
  const hit = transformHit(xf.value.t, hover.value, 10 * px.value)
  return hit.kind === 'scale' ? (hit.corner % 2 ? 'nesw-resize' : 'nwse-resize') : hit.kind === 'move' ? 'move' : 'alias'
})

const cursor = computed(() => {
  if (panning.value) return 'grabbing'
  if (spaceDown.value || props.tool === 'hand') return 'grab'
  if (xf.value) return xfCursor.value || 'default'
  if (props.tool === 'zoom') return altDown.value ? 'zoom-out' : 'zoom-in'
  if (props.tool === 'patch' && patchSel.value && hover.value && pointInPolygon(hover.value.x, hover.value.y, patchSel.value.points)) return 'move'
  return { move: 'move', text: 'text', eyedropper: 'crosshair', wand: 'crosshair', rect: 'crosshair', lasso: 'crosshair', crop: 'crosshair', gradient: 'crosshair', shape: 'crosshair', pen: 'crosshair', objectSelect: 'crosshair', patch: 'crosshair' }[props.tool] || 'none'
})
const showBrush = computed(() => (PAINTING.includes(props.tool) || PATHED.includes(props.tool)) && hover.value && !spaceDown.value && !xf.value)
const source = computed(() => (props.tool === 'clone' ? cloneSource.value : props.tool === 'heal' ? healSource.value : null))
</script>

<template>
  <div ref="stage" class="stage" :style="{ cursor }" @pointerdown="onDown" @pointermove="onMove" @pointerup="onUp" @pointercancel="onUp" @pointerleave="onLeave" @wheel="onWheel">
    <div v-if="doc" class="board" :style="boardStyle">
      <canvas ref="display" class="display"></canvas>
      <canvas v-show="quickMask" ref="quickView" class="quick"></canvas>
      <svg class="overlay" :viewBox="`0 0 ${W} ${H}`" preserveAspectRatio="none" aria-hidden="true">
        <path v-if="activePathD" class="kept-path" :d="activePathD" />
        <rect v-if="temp?.kind === 'object'" class="guide" :x="Math.min(temp.x0, temp.x1)" :y="Math.min(temp.y0, temp.y1)" :width="Math.abs(temp.x1 - temp.x0)" :height="Math.abs(temp.y1 - temp.y0)" />
        <rect v-if="textBoxDrag" class="guide" :x="textBoxDrag.x" :y="textBoxDrag.y" :width="textBoxDrag.w" :height="textBoxDrag.h" />
        <template v-if="patchPath">
          <path class="ants-back" :d="patchPath" />
          <path class="ants" :d="patchPath" />
        </template>
        <path v-if="patchMoved" class="guide" :d="patchMoved" />
        <path v-if="cropShade" class="shade" :d="cropShade" fill-rule="evenodd" />
        <path v-if="cropRect" class="crop-edge" :d="rectPath(cropRect)" />
        <template v-if="antsPath">
          <path class="ants-back" :d="antsPath" />
          <path class="ants" :d="antsPath" />
        </template>
        <line v-if="gradLine" class="guide" :x1="gradLine.from.x" :y1="gradLine.from.y" :x2="gradLine.to.x" :y2="gradLine.to.y" />
        <template v-if="shapePreview">
          <rect v-if="shapePreview.kind === 'rect'" class="guide" :x="shapePreview.x" :y="shapePreview.y" :width="shapePreview.w" :height="shapePreview.h" />
          <ellipse v-else-if="shapePreview.kind === 'ellipse'" class="guide" :cx="shapePreview.x + shapePreview.w / 2" :cy="shapePreview.y + shapePreview.h / 2" :rx="shapePreview.w / 2" :ry="shapePreview.h / 2" />
          <path v-else class="guide" :d="pathOf(shapePreview.points, false)" />
        </template>
        <path v-if="polyPath" class="guide" :d="polyPath" />
        <template v-if="penPath">
          <path class="guide pen" :d="penPath" />
          <rect v-for="(q, i) in pen.points" :key="i" class="pen-pt" :x="q[0] - 3 * px" :y="q[1] - 3 * px" :width="6 * px" :height="6 * px" />
        </template>
        <path v-if="healPath" class="heal-path" :d="pathOf(healPath.points, false)" :style="{ strokeWidth: brush.size }" />
        <circle v-if="showBrush" class="brush-ring" :cx="hover.x" :cy="hover.y" :r="Math.max(1, brush.size / 2)" />
        <g v-if="source" class="source">
          <path :d="`M${source.x - 8 * px} ${source.y}h${16 * px}M${source.x} ${source.y - 8 * px}v${16 * px}`" />
        </g>
        <TransformBox v-if="xf" :t="xf.t" :px="px" />
      </svg>
      <textarea
        v-if="editing"
        ref="textBox"
        v-model="editing.value"
        class="text-edit"
        rows="1"
        :style="textStyle"
        aria-label="Text"
        spellcheck="false"
        @keydown="onTextKey"
        @pointerdown.stop
        @blur="commitText"
      ></textarea>
    </div>
    <CompanionCursor ref="companionCursor" class="companion" :zoom="scale" :offset="boardPos" :name="companion" />
    <div v-if="cropRect && !cropDrag" class="crop-bar" @pointerdown.stop>
      <span>{{ cropRect.w }} × {{ cropRect.h }}</span>
      <button type="button" class="on" @click="applyCrop">Crop (Enter)</button>
      <button type="button" @click="cancelCrop">Cancel (Esc)</button>
    </div>
    <div v-if="xf" class="crop-bar" @pointerdown.stop>
      <span>Free transform</span>
      <button type="button" class="on" @click="commitTransform">Apply (Enter)</button>
      <button type="button" @click="cancelTransform">Cancel (Esc)</button>
    </div>
    <div v-if="pen" class="crop-bar" @pointerdown.stop>
      <span>{{ pen.points.length }} points</span>
      <button type="button" class="on" @click="penToSelection">Selection (Enter)</button>
      <button type="button" @click="penToShape">Shape (Shift+Enter)</button>
      <button type="button" @click="keepPath">Keep path (Ctrl+Enter)</button>
    </div>
    <div v-if="patchSel && !patchDrag && tool === 'patch'" class="crop-bar" @pointerdown.stop>
      <span>Drag the area onto a clean part</span>
      <button type="button" @click="patchSel = null">Cancel (Esc)</button>
    </div>
    <div v-if="quickMask" class="mask-chip quick-chip">Quick mask - paint to select, erase to take away. Q ends it.</div>
    <div v-if="channel" class="mask-chip">Showing the {{ { r: 'red', g: 'green', b: 'blue', alpha: 'alpha' }[channel] }} channel - RGB in CHANNELS goes back</div>
    <div v-if="editMask && selectedLayer()?.mask" class="mask-chip">Painting the mask - white shows, black hides</div>
    <div v-if="viewMask" class="mask-chip">Showing the mask alone - Alt+click its thumbnail to go back</div>
    <p v-if="working" class="working">Working…</p>
    <button v-if="doc && (zoom !== 1 || pan.x || pan.y)" type="button" class="fit" title="Fit the photo to the window" @pointerdown.stop @click="resetView">
      {{ Math.round(scale * 100) }}% · Fit
    </button>
    <slot />
  </div>
</template>

<style scoped>
.stage {
  position: relative;
  width: 100%;
  height: 100%;
  overflow: hidden;
  touch-action: none;
  user-select: none;
}

.board {
  position: absolute;
  transform-origin: 0 0;
  background: repeating-conic-gradient(#d9d6e4 0 25%, #ffffff 0 50%) 0 0 / 16px 16px;
  box-shadow: 0 6px 30px rgba(0, 0, 0, 0.45);
}

.display,
.overlay {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
}

.display {
  image-rendering: auto;
}

.quick {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  opacity: 0.5;
  pointer-events: none;
}

.kept-path {
  fill: rgba(62, 230, 240, 0.08);
  stroke: #3ee6f0;
  stroke-width: 1.5;
  vector-effect: non-scaling-stroke;
}

.quick-chip {
  bottom: 44px;
  border-color: rgba(255, 40, 60, 0.6);
}

.overlay {
  overflow: visible;
  pointer-events: none;
}

.companion {
  position: absolute;
  inset: 0;
  pointer-events: none;
}

.ants,
.ants-back,
.crop-edge,
.brush-ring,
.guide,
.source path {
  fill: none;
  vector-effect: non-scaling-stroke;
}

.ants-back {
  stroke: #000;
  stroke-width: 1;
}

.ants {
  stroke: #fff;
  stroke-width: 1;
  stroke-dasharray: 5 5;
  animation: march 0.6s linear infinite;
}

@keyframes march {
  to {
    stroke-dashoffset: -10;
  }
}

@media (prefers-reduced-motion: reduce) {
  .ants {
    animation: none;
  }
}

.shade {
  fill: rgba(8, 4, 24, 0.55);
}

.crop-edge {
  stroke: #fff;
  stroke-width: 1.5;
}

.guide {
  stroke: var(--accent, #a98bff);
  stroke-width: 1.5;
  stroke-dasharray: 6 4;
  filter: drop-shadow(0 0 1px #000);
}

.guide.pen {
  stroke-dasharray: none;
}

.pen-pt {
  fill: #fff;
  stroke: var(--accent, #a98bff);
  stroke-width: 1;
  vector-effect: non-scaling-stroke;
}

.heal-path {
  fill: none;
  stroke: color-mix(in srgb, var(--accent, #a98bff) 45%, transparent);
  stroke-linecap: round;
  stroke-linejoin: round;
}

.brush-ring {
  stroke: rgba(255, 255, 255, 0.9);
  stroke-width: 1;
  filter: drop-shadow(0 0 1px #000);
}

.source path {
  stroke: var(--accent, #8a5cf0);
  stroke-width: 1.5;
}

.text-edit {
  position: absolute;
  margin: 0;
  padding: 0;
  border: 1px dashed var(--accent, #8a5cf0);
  background: rgba(0, 0, 0, 0.12);
  outline: none;
  resize: none;
  overflow: hidden;
  white-space: pre;
  field-sizing: content;
}

.crop-bar {
  position: absolute;
  top: 10px;
  left: 50%;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: var(--panel-background, rgba(24, 19, 66, 0.9));
  backdrop-filter: blur(var(--glass-blur, 8px));
  color: var(--text);
  font-size: 12px;
  transform: translateX(-50%);
}

.crop-bar button,
.fit {
  padding: 4px 12px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}

.crop-bar button.on {
  border-color: var(--accent);
  background: var(--accent);
  color: #160f33;
}

.mask-chip,
.working {
  position: absolute;
  left: 10px;
  bottom: 10px;
  margin: 0;
  padding: 4px 12px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: var(--panel-background, rgba(24, 19, 66, 0.9));
  color: var(--text-dim);
  font-size: 12px;
  pointer-events: none;
}

.working {
  left: auto;
  right: 50%;
  bottom: auto;
  top: 10px;
  transform: translateX(50%);
}

.fit {
  position: absolute;
  right: 10px;
  bottom: 10px;
  background: var(--panel-background, rgba(24, 19, 66, 0.9));
  color: var(--text-dim);
}
</style>
