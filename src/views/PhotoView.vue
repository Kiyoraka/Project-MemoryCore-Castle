<script setup>
// PHOTO: a basic photo editor with the companion's chat (his pick at the Keep): a classic tool
// column, the photo in the middle, LAYERS and ADJUST on the right, a strip of undo pills under the
// photo, and a chat bar along the bottom. Every edit happens here, in the page; the core keeps the
// projects (media-generation/photo/<name>/), asks the member's own CLI once per chat line, and puts
// the redrawing edits on the image queue. Each edit is one pill, and each pill undoes itself alone.
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, shallowRef, watch } from 'vue'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { imageDismiss, imageJobs, photoAsk, photoExport, photoLoad, photoOpenDialog, photoRead, photoRecent, photoRedraw, photoResult, photoSave } from '../api/castle.js'
import { pictureCost } from '../components/appearance.js'
import GlassSelect from '../components/GlassSelect.vue'
import { ADJUST_KEYS, ADJUST_LABELS, adjustLabel, applyAdjustOp, neutralAdjust } from '../components/photo/adjust.js'
import { BLENDS, MAX_SIDE, REDRAWS, REDRAW_LABELS, changeLayer, cleanOp, cleanOps, cropDoc, docWithoutPixels, fieldsOf, flipDoc, imageLayer, insertLayer, moveLayer, newDoc, newId, readDoc, redrawLine, removeLayer, resizeDoc, rotateDoc, textLayer } from '../components/photo/doc.js'
import { addPill, pillLabel, undoLast, undoPill } from '../components/photo/history.js'
import { cleanLasso, dragRect, floodMask, lassoBounds, maskBounds, selectionBox } from '../components/photo/select.js'
import { antsPath, cloneDab, copyCanvas, dab, dabPoints, flatten, flipCanvas, jpegOf, loadCanvas, makeCanvas, pngOf, renderComposite, rotateCanvas, scaleCanvas, selectionPath, shrink, textBox } from '../components/photo/canvas.js'

const props = defineProps({
  companion: { type: String, default: 'Your companion' },
  cli: { type: String, default: 'claude' },
  visible: { type: Boolean, default: true },
})

const TOOLS = [
  { id: 'move', key: 'V', label: 'Move', d: 'M12 3v18M3 12h18M12 3 9 6M12 3l3 3M12 21l-3-3M12 21l3-3M3 12l3-3M3 12l3 3M21 12l-3-3M21 12l-3 3' },
  { id: 'marquee', key: 'M', label: 'Rectangle select', d: 'M4 4h4M12 4h4M20 4v4M20 12v4M20 20h-4M12 20H8M4 20v-4M4 12V8' },
  { id: 'lasso', key: 'L', label: 'Lasso', d: 'M12 4c5 0 9 2.5 9 6s-4 6-9 6-9-2.5-9-6 4-6 9-6ZM6 15c-1.5 2-.5 4.5 2 5' },
  { id: 'wand', key: 'W', label: 'Magic wand', d: 'M4 20 15 9M15 3v3M21 9h-3M19 5l-2 2M11 5l1.5 1.5M19 13l-1.5-1.5' },
  { id: 'crop', key: 'C', label: 'Crop', d: 'M6 2v16h16M2 6h16v16' },
  { id: 'brush', key: 'B', label: 'Brush', d: 'M18 3l3 3-9 9-3-3ZM9 12c-3 0-5 2-5 5 0 1-1 2-2 2 2 2 7 2 8-1 .7-2 0-4-1-6' },
  { id: 'eraser', key: 'E', label: 'Eraser', d: 'M8 21h13M3 16l9-9 6 6-5 5H8Z' },
  { id: 'clone', key: 'S', label: 'Clone stamp (Alt+click the source)', d: 'M9 3h6v5l3 3v3H6v-3l3-3ZM5 18h14v3H5Z' },
  { id: 'text', key: 'T', label: 'Text', d: 'M5 5h14M12 5v15M9 20h6' },
  { id: 'eyedropper', key: 'I', label: 'Eyedropper', d: 'M19 5a2.8 2.8 0 0 0-4 0l-2 2-1-1-2 2 1 1-7 7v3h3l7-7 1 1 2-2-1-1 2-2a2.8 2.8 0 0 0 0-4Z' },
]
const PAINTS = ['brush', 'eraser', 'clone']
const REDRAW_USD = pictureCost(null, 'medium')

// --- the document and its pixels ---------------------------------------------------------------
const doc = shallowRef(null)
// { [layerId]: canvas } - never changed in place: an edit puts in a new canvas (a pill keeps the old)
let pixels = {}
const version = ref(0)
const history = shallowRef([])
let redoStack = []
const name = ref('')
const dirty = ref(false)
const activeId = ref(null)
const tool = ref('move')
const opts = reactive({ size: 30, hardness: 70, opacity: 100, color: '#ff5d8f', tolerance: 32, contiguous: true, textSize: 64 })
const view = reactive({ zoom: 1, panX: 0, panY: 0 })
const cropRect = ref(null)
const notice = ref('')
const problem = ref('')
const busy = ref(false)
const thumbs = ref({})
const recent = ref([])
const menu = ref('') // 'recent' | 'export' | 'resize'
const exportKind = ref('png')
const exportQuality = ref(90)
const resizeTo = reactive({ width: 0, height: 0, keep: true })
const dropping = ref(false)
let confirmDiscard = 0

const active = computed(() => doc.value?.layers.find((l) => l.id === activeId.value) ?? null)
const layersTopFirst = computed(() => (doc.value ? [...doc.value.layers].reverse() : []))
const zoomLine = computed(() => `${Math.round(view.zoom * 100)}%`)

function say(text) {
  notice.value = text
  problem.value = ''
}
function fail(e) {
  problem.value = String(e?.message ?? e)
}

// --- drawing: the composite (doc size) and the view (the screen) -------------------------------
const viewport = ref(null)
const viewCanvas = ref(null)
let comp = null
let compCtx = null
let fullDirty = true
let dirtyRect = null
let frameAsked = false
let hover = null
let marquee = null
let lasso = null
let cloneSource = null
let cloneOffset = null

function union(a, b) {
  const x = Math.min(a.x, b.x)
  const y = Math.min(a.y, b.y)
  return { x, y, w: Math.max(a.x + a.w, b.x + b.w) - x, h: Math.max(a.y + a.h, b.y + b.h) - y }
}
function markDirty(rect = null) {
  if (rect) dirtyRect = dirtyRect ? union(dirtyRect, rect) : rect
  else fullDirty = true
  askFrame()
}
function askFrame() {
  if (frameAsked || !props.visible) return
  frameAsked = true
  requestAnimationFrame(frame)
}
function animating() {
  return Boolean(doc.value && (doc.value.selection || cropRect.value || marquee || lasso))
}
function frame() {
  frameAsked = false
  const d = doc.value
  if (d) {
    if (!comp || comp.width !== d.width || comp.height !== d.height) {
      comp = makeCanvas(d.width, d.height)
      compCtx = comp.getContext('2d', { willReadFrequently: true })
      fullDirty = true
    }
    if (fullDirty) renderComposite(compCtx, d, pixels)
    else if (dirtyRect) renderComposite(compCtx, d, pixels, { rect: dirtyRect })
    fullDirty = false
    dirtyRect = null
  }
  drawView()
  if (animating()) setTimeout(askFrame, 90)
}

let checker = null
function checkerPattern(ctx) {
  if (checker) return checker
  const c = makeCanvas(16, 16)
  const g = c.getContext('2d')
  g.fillStyle = '#d9d9e3'
  g.fillRect(0, 0, 16, 16)
  g.fillStyle = '#b9b9c9'
  g.fillRect(0, 0, 8, 8)
  g.fillRect(8, 8, 8, 8)
  checker = ctx.createPattern(c, 'repeat')
  return checker
}

function drawView() {
  const el = viewCanvas.value
  const box = viewport.value
  if (!el || !box) return
  const dpr = window.devicePixelRatio || 1
  const w = Math.max(1, Math.round(box.clientWidth * dpr))
  const h = Math.max(1, Math.round(box.clientHeight * dpr))
  if (el.width !== w || el.height !== h) {
    el.width = w
    el.height = h
  }
  const ctx = el.getContext('2d')
  ctx.setTransform(1, 0, 0, 1, 0, 0)
  ctx.clearRect(0, 0, w, h)
  const d = doc.value
  if (!d || !comp) return
  const z = view.zoom * dpr
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
  // the checkerboard stays the same size on screen
  ctx.fillStyle = checkerPattern(ctx)
  ctx.fillRect(view.panX, view.panY, d.width * view.zoom, d.height * view.zoom)
  ctx.setTransform(z, 0, 0, z, view.panX * dpr, view.panY * dpr)
  ctx.imageSmoothingEnabled = view.zoom < 2
  ctx.drawImage(comp, 0, 0)
  const px = 1 / view.zoom
  const offset = (performance.now() / 60) % 8
  const ants = (path) => {
    ctx.lineWidth = px
    ctx.setLineDash([4 * px, 4 * px])
    ctx.strokeStyle = '#000'
    ctx.lineDashOffset = -offset * px
    ctx.stroke(path)
    ctx.strokeStyle = '#fff'
    ctx.lineDashOffset = (4 - offset) * px
    ctx.stroke(path)
    ctx.setLineDash([])
  }
  if (tool.value === 'move' && active.value) {
    const b = layerBox(active.value)
    ctx.strokeStyle = 'rgba(183, 166, 255, 0.9)'
    ctx.lineWidth = px
    ctx.strokeRect(b.x, b.y, b.w, b.h)
  }
  if (d.selection) ants(antsPath(d.selection))
  if (marquee) {
    const p = new Path2D()
    p.rect(marquee.x, marquee.y, marquee.w, marquee.h)
    ants(p)
  }
  if (lasso && lasso.length > 1) {
    const p = new Path2D()
    lasso.forEach(([x, y], i) => (i ? p.lineTo(x, y) : p.moveTo(x, y)))
    ants(p)
  }
  if (cropRect.value) {
    const c = cropRect.value
    ctx.fillStyle = 'rgba(0, 0, 0, 0.5)'
    ctx.beginPath()
    ctx.rect(0, 0, d.width, d.height)
    ctx.rect(c.x, c.y, c.w, c.h)
    ctx.fill('evenodd')
    ctx.strokeStyle = '#fff'
    ctx.lineWidth = px * 1.5
    ctx.strokeRect(c.x, c.y, c.w, c.h)
  }
  if (tool.value === 'clone' && cloneSource) {
    ctx.strokeStyle = '#fff'
    ctx.lineWidth = px * 1.5
    const s = cloneSource
    ctx.beginPath()
    ctx.moveTo(s.x - 8 * px, s.y)
    ctx.lineTo(s.x + 8 * px, s.y)
    ctx.moveTo(s.x, s.y - 8 * px)
    ctx.lineTo(s.x, s.y + 8 * px)
    ctx.stroke()
  }
  if (hover && PAINTS.includes(tool.value)) {
    ctx.strokeStyle = 'rgba(255, 255, 255, 0.9)'
    ctx.lineWidth = px
    ctx.beginPath()
    ctx.arc(hover.x, hover.y, opts.size / 2, 0, Math.PI * 2)
    ctx.stroke()
    ctx.strokeStyle = 'rgba(0, 0, 0, 0.6)'
    ctx.beginPath()
    ctx.arc(hover.x, hover.y, opts.size / 2 + px, 0, Math.PI * 2)
    ctx.stroke()
  }
}

/** A layer's box in the picture: its pixels' size, or its text measured. */
function layerBox(layer) {
  if (layer.kind === 'text') {
    const t = textBox(compCtx ?? makeCanvas(1, 1).getContext('2d'), layer)
    return { x: layer.x, y: layer.y, w: t.w, h: t.h }
  }
  const c = pixels[layer.id]
  return { x: layer.x, y: layer.y, w: c?.width ?? 0, h: c?.height ?? 0 }
}

function fit() {
  const d = doc.value
  const box = viewport.value
  if (!d || !box) return
  const z = Math.min((box.clientWidth - 40) / d.width, (box.clientHeight - 40) / d.height, 2)
  view.zoom = Math.max(0.02, z)
  view.panX = (box.clientWidth - d.width * view.zoom) / 2
  view.panY = (box.clientHeight - d.height * view.zoom) / 2
  askFrame()
}

function zoomAt(factor, sx, sy) {
  const z = Math.min(32, Math.max(0.02, view.zoom * factor))
  view.panX = sx - ((sx - view.panX) * z) / view.zoom
  view.panY = sy - ((sy - view.panY) * z) / view.zoom
  view.zoom = z
  askFrame()
}

// --- the edits: every one is one pill -----------------------------------------------------------
function commit(nextDoc, pill, nextPixels = null) {
  doc.value = nextDoc
  if (nextPixels) pixels = nextPixels
  if (pill) history.value = addPill(history.value, { ...pill, label: pillLabel(pill.label) })
  redoStack = []
  dirty.value = true
  if (!nextDoc.layers.some((l) => l.id === activeId.value)) activeId.value = nextDoc.layers.at(-1)?.id ?? null
  version.value += 1
  markDirty()
}

function restore(state, nextHistory) {
  doc.value = state.doc
  pixels = state.pixels
  history.value = nextHistory
  dirty.value = true
  if (!state.doc.layers.some((l) => l.id === activeId.value)) activeId.value = state.doc.layers.at(-1)?.id ?? null
  cropRect.value = null
  version.value += 1
  markDirty()
}

function undoOne(id) {
  const r = undoPill({ doc: doc.value, pixels }, history.value, id)
  redoStack = []
  restore(r.state, r.history)
}
function undo() {
  if (!history.value.length) return
  const r = undoLast({ doc: doc.value, pixels }, history.value)
  if (r.redo) redoStack.push(r.redo)
  restore(r.state, r.history)
}
function redo() {
  const r = redoStack.pop()
  if (r) restore(r.state, r.history)
}

function doAdjust(values, mode = 'relative', from = '') {
  const { next, before } = applyAdjustOp(doc.value.adjust, values, mode)
  if (!Object.keys(before).length) return
  commit({ ...doc.value, adjust: next }, { kind: 'adjust', before, label: `${from}${adjustLabel(before, next)}` })
}

// a slider dragged by hand: drawn live, one pill when it is let go
let sliderFrom = null
function sliderInput(key, value) {
  if (!doc.value) return
  if (sliderFrom === null) sliderFrom = { key, value: doc.value.adjust[key] }
  doc.value = { ...doc.value, adjust: { ...doc.value.adjust, [key]: Number(value) } }
  markDirty()
}
function sliderDone(key) {
  if (!sliderFrom || sliderFrom.key !== key) return
  const before = { [key]: sliderFrom.value }
  sliderFrom = null
  if (doc.value.adjust[key] === before[key]) return
  commit(doc.value, { kind: 'adjust', before, label: adjustLabel(before, doc.value.adjust) })
}
function resetAdjust() {
  const before = Object.fromEntries(ADJUST_KEYS.filter((k) => doc.value.adjust[k]).map((k) => [k, doc.value.adjust[k]]))
  if (!Object.keys(before).length) return
  commit({ ...doc.value, adjust: neutralAdjust() }, { kind: 'adjust', before, label: 'Reset adjust' })
}

function snapshot() {
  return { doc: doc.value, pixels: { ...pixels } }
}
function sizes() {
  return Object.fromEntries(doc.value.layers.map((l) => [l.id, layerBox(l)]).map(([id, b]) => [id, { w: b.w, h: b.h }]))
}
function doCrop(box, from = '') {
  if (!box) return
  const before = snapshot()
  commit(cropDoc(doc.value, box), { kind: 'snapshot', before, label: `${from}Crop ${box.w} x ${box.h}` })
  cropRect.value = null
  fit()
}
function doRotate(turns, from = '') {
  const before = snapshot()
  const next = {}
  for (const [id, c] of Object.entries(pixels)) next[id] = rotateCanvas(c, turns)
  const t = ((turns % 4) + 4) % 4
  commit(rotateDoc(doc.value, turns, sizes()), { kind: 'snapshot', before, label: `${from}Rotate ${t === 3 ? 'left' : t === 2 ? '180' : 'right'}` }, next)
  fit()
}
function doFlip(axis, from = '') {
  const before = snapshot()
  const next = {}
  for (const [id, c] of Object.entries(pixels)) next[id] = flipCanvas(c, axis)
  commit(flipDoc(doc.value, axis, sizes()), { kind: 'snapshot', before, label: `${from}Flip ${axis === 'h' ? 'across' : 'upside down'}` }, next)
}
function doResize(width, height, from = '') {
  const d = doc.value
  const w = Math.min(MAX_SIDE, Math.max(1, Math.round(width)))
  const h = Math.min(MAX_SIDE, Math.max(1, Math.round(height)))
  if (w === d.width && h === d.height) return
  const before = snapshot()
  const next = {}
  for (const [id, c] of Object.entries(pixels)) next[id] = scaleCanvas(c, w / d.width, h / d.height)
  commit(resizeDoc(d, w, h), { kind: 'snapshot', before, label: `${from}Resize ${w} x ${h}` }, next)
  fit()
}
function addText(value, x, y, size, color, from = '') {
  const layer = textLayer(value, { x, y, size, color })
  commit(insertLayer(doc.value, layer), { kind: 'layer-new', layerId: layer.id, shown: [], label: `${from}Text "${value}"` })
  activeId.value = layer.id
  return layer
}
function changeFields(id, fields, label) {
  const layer = doc.value.layers.find((l) => l.id === id)
  if (!layer) return
  const before = fieldsOf(layer, fields)
  if (Object.keys(fields).every((k) => JSON.stringify(layer[k]) === JSON.stringify(fields[k]))) return
  commit(changeLayer(doc.value, id, fields), { kind: 'layer-change', layerId: id, before, label })
}
function doSelect(op, from = '') {
  const before = doc.value.selection
  let selection = null
  if (op.all) selection = { kind: 'rect', x: 0, y: 0, w: doc.value.width, h: doc.value.height }
  else if (!op.none) selection = { kind: 'rect', x: op.x, y: op.y, w: op.w, h: op.h }
  commit({ ...doc.value, selection }, { kind: 'select', before, label: `${from}${selection ? 'Select' : 'Deselect'}` })
}
function setSelection(selection) {
  doc.value = { ...doc.value, selection }
  askFrame()
}

// --- layers -------------------------------------------------------------------------------------
function newLayer() {
  const layer = imageLayer(`Layer ${doc.value.layers.length + 1}`)
  commit(insertLayer(doc.value, layer, layerIndex(activeId.value) + 1), { kind: 'layer-new', layerId: layer.id, shown: [], label: 'New layer' }, { ...pixels, [layer.id]: makeCanvas(doc.value.width, doc.value.height) })
  activeId.value = layer.id
}
function layerIndex(id) {
  return doc.value.layers.findIndex((l) => l.id === id)
}
function duplicateLayer(layer) {
  const copy = { ...layer, id: newId(layer.kind === 'text' ? 't' : 'l'), name: `${layer.name} copy`.slice(0, 60), ...(layer.text ? { text: { ...layer.text } } : {}) }
  const nextPixels = pixels[layer.id] ? { ...pixels, [copy.id]: pixels[layer.id] } : pixels
  commit(insertLayer(doc.value, copy, layerIndex(layer.id) + 1), { kind: 'layer-new', layerId: copy.id, shown: [], label: `Duplicate ${layer.name}` }, nextPixels)
  activeId.value = copy.id
}
function deleteLayer(layer) {
  if (doc.value.layers.length <= 1) return say('A picture keeps at least one layer.')
  const index = layerIndex(layer.id)
  const nextPixels = { ...pixels }
  delete nextPixels[layer.id]
  commit(removeLayer(doc.value, layer.id), { kind: 'layer-delete', layer, index, pixel: pixels[layer.id] ?? null, label: `Delete ${layer.name}` }, nextPixels)
}
function moveLayerBy(layer, step) {
  const next = moveLayer(doc.value, layer.id, step)
  if (next === doc.value) return
  commit(next, { kind: 'order', before: doc.value.layers.map((l) => l.id), label: `Move ${layer.name} ${step > 0 ? 'up' : 'down'}` })
}
const renaming = ref(null)
const renameText = ref('')
function startRename(layer) {
  renaming.value = layer.id
  renameText.value = layer.name
  nextTick(() => document.getElementById(`rename-${layer.id}`)?.select())
}
function finishRename(layer) {
  if (renaming.value !== layer.id) return
  renaming.value = null
  const n = renameText.value.replace(/\s+/g, ' ').trim().slice(0, 60)
  if (n && n !== layer.name) changeFields(layer.id, { name: n }, `Rename to ${n}`)
}
// opacity dragged by hand: live, one pill when let go
let opacityFrom = null
function opacityInput(value) {
  const l = active.value
  if (!l) return
  if (opacityFrom === null) opacityFrom = { id: l.id, value: l.opacity }
  doc.value = changeLayer(doc.value, l.id, { opacity: Number(value) })
  markDirty()
}
function opacityDone() {
  if (!opacityFrom) return
  const { id, value } = opacityFrom
  opacityFrom = null
  const l = doc.value.layers.find((x) => x.id === id)
  if (!l || l.opacity === value) return
  commit(doc.value, { kind: 'layer-change', layerId: id, before: { opacity: value }, label: `${l.name} opacity ${l.opacity}%` })
}
// a text layer's words, size and colour: live, one pill when the field is left
let textFrom = null
function textInput(fields) {
  const l = active.value
  if (!l || l.kind !== 'text') return
  if (!textFrom) textFrom = { id: l.id, text: l.text, name: l.name }
  const text = { ...l.text, ...fields }
  // a text layer is named by its first line until the member renames it
  const auto = (value) => value.split('\n')[0].slice(0, 30) || 'Text'
  doc.value = changeLayer(doc.value, l.id, l.name === auto(l.text.value) ? { text, name: auto(text.value) } : { text })
  markDirty()
}
function textDone() {
  if (!textFrom) return
  const { id, text, name: oldName } = textFrom
  textFrom = null
  const l = doc.value.layers.find((x) => x.id === id)
  if (!l || JSON.stringify(l.text) === JSON.stringify(text)) return
  commit(doc.value, { kind: 'layer-change', layerId: id, before: { text, name: oldName }, label: `Edit text "${l.text.value}"` })
}

let thumbTimer = null
watch(version, () => {
  clearTimeout(thumbTimer)
  thumbTimer = setTimeout(() => {
    const out = {}
    for (const l of doc.value?.layers ?? []) if (pixels[l.id]) out[l.id] = shrink(pixels[l.id], 48).toDataURL('image/png')
    thumbs.value = out
  }, 150)
})

// --- the pointer on the photo -------------------------------------------------------------------
let drag = null
let spaceDown = false

function toDoc(e) {
  const r = viewCanvas.value.getBoundingClientRect()
  return { x: (e.clientX - r.left - view.panX) / view.zoom, y: (e.clientY - r.top - view.panY) / view.zoom, sx: e.clientX - r.left, sy: e.clientY - r.top }
}

function pointerDown(e) {
  if (!doc.value) return
  viewCanvas.value.setPointerCapture(e.pointerId)
  const p = toDoc(e)
  if (spaceDown || e.button === 1) {
    drag = { kind: 'pan', sx: p.sx, sy: p.sy, panX: view.panX, panY: view.panY }
    return
  }
  if (e.button !== 0) return
  const t = tool.value
  const d = doc.value
  if (t === 'move') {
    const l = active.value
    if (l) drag = { kind: 'move', x0: p.x, y0: p.y, lx: l.x, ly: l.y, id: l.id }
  } else if (t === 'marquee' || t === 'crop') {
    drag = { kind: t, x0: p.x, y0: p.y }
  } else if (t === 'lasso') {
    lasso = [[p.x, p.y]]
    drag = { kind: 'lasso' }
  } else if (t === 'wand') {
    if (p.x < 0 || p.y < 0 || p.x >= d.width || p.y >= d.height) return
    const data = compCtx.getImageData(0, 0, d.width, d.height).data
    const mask = floodMask(data, d.width, d.height, p.x, p.y, opts.tolerance, opts.contiguous)
    const bounds = maskBounds(mask, d.width, d.height)
    setSelection(bounds ? { kind: 'mask', mask, w: d.width, h: d.height, bounds } : null)
  } else if (PAINTS.includes(t)) {
    startStroke(p, e)
  } else if (t === 'text') {
    const hit = [...d.layers].reverse().find((l) => {
      if (l.kind !== 'text' || !l.visible) return false
      const b = layerBox(l)
      return p.x >= b.x && p.y >= b.y && p.x <= b.x + b.w && p.y <= b.y + b.h
    })
    if (hit) activeId.value = hit.id
    else addText('Text', Math.round(p.x), Math.round(p.y), opts.textSize, opts.color)
    nextTick(() => document.getElementById('photo-text-words')?.select())
  } else if (t === 'eyedropper') {
    if (p.x < 0 || p.y < 0 || p.x >= d.width || p.y >= d.height) return
    const [r, g, b] = compCtx.getImageData(Math.floor(p.x), Math.floor(p.y), 1, 1).data
    opts.color = `#${[r, g, b].map((v) => v.toString(16).padStart(2, '0')).join('')}`
    say(`Colour picked: ${opts.color}`)
  }
}

function pointerMove(e) {
  if (!doc.value) return
  const p = toDoc(e)
  hover = { x: p.x, y: p.y }
  if (!drag) return askFrame()
  const d = doc.value
  if (drag.kind === 'pan') {
    view.panX = drag.panX + p.sx - drag.sx
    view.panY = drag.panY + p.sy - drag.sy
  } else if (drag.kind === 'move') {
    const x = Math.round(drag.lx + p.x - drag.x0)
    const y = Math.round(drag.ly + p.y - drag.y0)
    doc.value = changeLayer(d, drag.id, { x, y })
    markDirty()
  } else if (drag.kind === 'marquee') {
    marquee = dragRect(drag.x0, drag.y0, p.x, p.y, d.width, d.height)
  } else if (drag.kind === 'crop') {
    cropRect.value = dragRect(drag.x0, drag.y0, p.x, p.y, d.width, d.height)
  } else if (drag.kind === 'lasso') {
    const last = lasso[lasso.length - 1]
    if (Math.hypot(p.x - last[0], p.y - last[1]) * view.zoom > 3) lasso.push([p.x, p.y])
  } else if (drag.kind === 'stroke') {
    continueStroke(p)
  }
  askFrame()
}

function pointerUp() {
  if (!drag || !doc.value) {
    drag = null
    return
  }
  const d = doc.value
  if (drag.kind === 'move') {
    const l = d.layers.find((x) => x.id === drag.id)
    if (l && (l.x !== drag.lx || l.y !== drag.ly)) commit(d, { kind: 'layer-change', layerId: l.id, before: { x: drag.lx, y: drag.ly }, label: `Move ${l.name}` })
  } else if (drag.kind === 'marquee') {
    setSelection(marquee ? { kind: 'rect', ...marquee } : null)
    marquee = null
  } else if (drag.kind === 'lasso') {
    const points = cleanLasso(lasso, d.width, d.height)
    setSelection(points ? { kind: 'lasso', points, bounds: lassoBounds(points) } : null)
    lasso = null
  } else if (drag.kind === 'stroke') {
    endStroke()
  }
  drag = null
  askFrame()
}

function leaveView() {
  hover = null
  askFrame()
}

function wheel(e) {
  if (!doc.value) return
  e.preventDefault()
  const p = toDoc(e)
  if (e.ctrlKey || e.metaKey) zoomAt(e.deltaY < 0 ? 1.15 : 1 / 1.15, p.sx, p.sy)
  else {
    view.panX -= e.shiftKey ? e.deltaY : e.deltaX
    view.panY -= e.shiftKey ? 0 : e.deltaY
    askFrame()
  }
}

// --- the brush, the eraser and the clone stamp --------------------------------------------------
let stroke = null
function startStroke(p, e) {
  const l = active.value
  if (!l || l.kind !== 'image' || !pixels[l.id]) return say('Choose an image layer to paint on.')
  if (!l.visible) return say('That layer is hidden; show it to paint on it.')
  if (tool.value === 'clone') {
    if (e.altKey) {
      cloneSource = { x: p.x, y: p.y }
      cloneOffset = null
      say('Source chosen. Now paint where it should be copied.')
      return askFrame()
    }
    if (!cloneSource) return say('Alt+click where to copy from first.')
    if (!cloneOffset) cloneOffset = { x: cloneSource.x - p.x, y: cloneSource.y - p.y }
  }
  const base = pixels[l.id]
  const live = copyCanvas(base)
  pixels = { ...pixels, [l.id]: live }
  const mark = makeCanvas(base.width, base.height)
  const ctx = mark.getContext('2d')
  // the stroke is drawn in picture coordinates, clipped to the selection
  ctx.translate(-l.x, -l.y)
  const sel = doc.value.selection
  if (sel) ctx.clip(selectionPath(sel))
  stroke = { layer: l, base, live, mark, ctx, last: { x: p.x, y: p.y }, tool: tool.value }
  drag = { kind: 'stroke' }
  stamp([{ x: p.x, y: p.y }])
}
function stamp(points) {
  const s = stroke
  const size = opts.size
  for (const q of points) {
    if (s.tool === 'clone') cloneDab(s.ctx, s.base, q.x, q.y, cloneOffset.x - s.layer.x, cloneOffset.y - s.layer.y, size, opts.hardness)
    else dab(s.ctx, q.x, q.y, size, opts.hardness, s.tool === 'eraser' ? '#000000' : opts.color)
  }
  const xs = points.map((q) => q.x)
  const ys = points.map((q) => q.y)
  const r = size / 2 + 2
  const rect = { x: Math.min(...xs) - r, y: Math.min(...ys) - r, w: Math.max(...xs) - Math.min(...xs) + 2 * r, h: Math.max(...ys) - Math.min(...ys) + 2 * r }
  // the live layer: as it began, with the stroke laid on at the brush's opacity
  const lx = Math.floor(rect.x - s.layer.x)
  const ly = Math.floor(rect.y - s.layer.y)
  const lw = Math.ceil(rect.w) + 1
  const lh = Math.ceil(rect.h) + 1
  const g = s.live.getContext('2d')
  g.save()
  g.beginPath()
  g.rect(lx, ly, lw, lh)
  g.clip()
  g.clearRect(lx, ly, lw, lh)
  g.drawImage(s.base, 0, 0)
  g.globalAlpha = opts.opacity / 100
  g.globalCompositeOperation = s.tool === 'eraser' ? 'destination-out' : 'source-over'
  g.drawImage(s.mark, 0, 0)
  g.restore()
  markDirty(rect)
}
function continueStroke(p) {
  if (!stroke) return
  stamp(dabPoints(stroke.last, p, opts.size))
  stroke.last = { x: p.x, y: p.y }
}
function endStroke() {
  if (!stroke) return
  const { layer, base, tool: t } = stroke
  stroke = null
  const label = t === 'eraser' ? 'Eraser' : t === 'clone' ? 'Clone stamp' : 'Brush'
  commit(doc.value, { kind: 'pixels', layerId: layer.id, before: base, label: `${label} on ${layer.name}` })
}

function clearSelected() {
  const l = active.value
  const sel = doc.value?.selection
  if (!l || !sel || l.kind !== 'image' || !pixels[l.id]) return
  const base = pixels[l.id]
  const next = copyCanvas(base)
  const g = next.getContext('2d')
  g.translate(-l.x, -l.y)
  g.clip(selectionPath(sel))
  g.clearRect(l.x, l.y, next.width, next.height)
  commit(doc.value, { kind: 'pixels', layerId: l.id, before: base, label: `Clear on ${l.name}` }, { ...pixels, [l.id]: next })
}

function applyCrop() {
  const box = cropRect.value ?? (doc.value?.selection ? selectionBox(doc.value.selection) : null)
  if (box) doCrop({ x: box.x, y: box.y, w: box.w, h: box.h })
}

// --- the keyboard -------------------------------------------------------------------------------
function typing(e) {
  const t = e.target
  return t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable)
}
function keyDown(e) {
  if (!props.visible || !doc.value) {
    if (props.visible && (e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'o') {
      e.preventDefault()
      openPicture()
    }
    return
  }
  if (typing(e)) return
  const k = e.key.toLowerCase()
  const ctrl = e.ctrlKey || e.metaKey
  if (ctrl) {
    if (k === 'z' && !e.shiftKey) undo()
    else if (k === 'y' || (k === 'z' && e.shiftKey)) redo()
    else if (k === 'a') setSelection({ kind: 'rect', x: 0, y: 0, w: doc.value.width, h: doc.value.height })
    else if (k === 'd') setSelection(null)
    else if (k === 's') save()
    else if (k === 'o') openPicture()
    else return
    e.preventDefault()
    return
  }
  if (e.key === ' ') {
    spaceDown = true
    e.preventDefault()
    return
  }
  if (e.key === 'Enter' && tool.value === 'crop') return applyCrop()
  if (e.key === 'Escape') {
    cropRect.value = null
    lasso = null
    marquee = null
    return askFrame()
  }
  if (e.key === 'Delete' || e.key === 'Backspace') return clearSelected()
  if (e.key === '[') opts.size = Math.max(1, Math.round(opts.size / 1.2))
  else if (e.key === ']') opts.size = Math.min(500, Math.round(opts.size * 1.2) + 1)
  else {
    const t = TOOLS.find((x) => x.key.toLowerCase() === k)
    if (!t) return
    tool.value = t.id
  }
  askFrame()
}
function keyUp(e) {
  if (e.key === ' ') spaceDown = false
}

// --- files --------------------------------------------------------------------------------------
function guard() {
  if (!dirty.value || Date.now() < confirmDiscard) return true
  confirmDiscard = Date.now() + 5000
  say('This picture has unsaved changes. Press again within five seconds to leave them.')
  return false
}

async function startFrom(canvas, title) {
  let c = canvas
  if (Math.max(c.width, c.height) > MAX_SIDE) {
    const k = MAX_SIDE / Math.max(c.width, c.height)
    c = scaleCanvas(c, k, k)
    say(`The picture was larger than ${MAX_SIDE} pixels, so it opened smaller.`)
  }
  const d = newDoc(c.width, c.height)
  const layer = imageLayer('Background')
  d.layers = [layer]
  pixels = { [layer.id]: c }
  doc.value = d
  history.value = []
  redoStack = []
  activeId.value = layer.id
  name.value = (title ?? '').replace(/[^\p{L}\p{N} _-]+/gu, ' ').trim().slice(0, 60) || 'Picture'
  dirty.value = false
  cropRect.value = null
  cloneSource = null
  cloneOffset = null
  version.value += 1
  await nextTick()
  fit()
  markDirty()
}

async function openPicture() {
  if (!guard()) return
  menu.value = ''
  try {
    const got = await photoOpenDialog()
    if (got) await startFrom(await loadCanvas(got.dataUrl), got.name)
  } catch (e) {
    fail(e)
  }
}

async function addPictureLayer(got) {
  let c = await loadCanvas(got.dataUrl)
  const d = doc.value
  const k = Math.min(1, d.width / c.width, d.height / c.height)
  if (k < 1) c = scaleCanvas(c, k, k)
  const layer = imageLayer(String(got.name || 'Picture').slice(0, 60), { x: Math.round((d.width - c.width) / 2), y: Math.round((d.height - c.height) / 2) })
  commit(insertLayer(d, layer), { kind: 'layer-new', layerId: layer.id, shown: [], label: `Add ${layer.name}` }, { ...pixels, [layer.id]: c })
  activeId.value = layer.id
  say('Dropped in as a new layer.')
}

async function onDragDrop(event) {
  if (!props.visible) return
  const drop = event.payload
  if (drop.type === 'enter' || drop.type === 'over') dropping.value = true
  else if (drop.type === 'leave') dropping.value = false
  else if (drop.type === 'drop') {
    dropping.value = false
    const path = drop.paths?.[0]
    if (!path) return
    try {
      const got = await photoRead(path)
      if (doc.value) await addPictureLayer(got)
      else await startFrom(await loadCanvas(got.dataUrl), got.name)
    } catch (e) {
      fail(e)
    }
  }
}

async function save() {
  if (!doc.value) return
  const title = name.value.trim()
  if (!title) return say('Give the picture a name first.')
  busy.value = true
  try {
    const d = doc.value
    const layers = d.layers.filter((l) => l.kind === 'image' && pixels[l.id]).map((l) => ({ id: l.id, png: pngOf(pixels[l.id]) }))
    const thumb = pngOf(shrink(flatten(d, pixels), 256))
    const kept = await photoSave(title, { ...docWithoutPixels(d), selection: selectionBox(d.selection) }, layers, thumb)
    name.value = kept
    dirty.value = false
    say(`Saved in your memory folder, media-generation/photo/${kept}/.`)
  } catch (e) {
    fail(e)
  } finally {
    busy.value = false
  }
}

async function showRecent() {
  menu.value = menu.value === 'recent' ? '' : 'recent'
  if (menu.value !== 'recent') return
  try {
    recent.value = await photoRecent()
  } catch (e) {
    fail(e)
  }
}

async function openRecent(item) {
  if (!guard()) return
  menu.value = ''
  busy.value = true
  try {
    const got = await photoLoad(item.name)
    const d = readDoc(got.doc)
    const next = {}
    for (const l of d.layers) {
      if (l.kind === 'image') next[l.id] = got.pngs[l.id] ? await loadCanvas(got.pngs[l.id]) : makeCanvas(d.width, d.height)
    }
    pixels = next
    doc.value = d
    history.value = []
    redoStack = []
    activeId.value = d.layers.at(-1)?.id ?? null
    name.value = got.name
    dirty.value = false
    cropRect.value = null
    version.value += 1
    await nextTick()
    fit()
    markDirty()
    say(`Opened ${got.name}.`)
  } catch (e) {
    fail(e)
  } finally {
    busy.value = false
  }
}

async function exportPicture() {
  if (!doc.value) return
  busy.value = true
  try {
    const flat = flatten(doc.value, pixels)
    const data = exportKind.value === 'jpg' ? await jpegOf(flat, exportQuality.value / 100) : pngOf(flat)
    const path = await photoExport(name.value || 'Picture', exportKind.value, data)
    if (path) {
      menu.value = ''
      say(`Exported to ${path}.`)
    }
  } catch (e) {
    fail(e)
  } finally {
    busy.value = false
  }
}

function openResize() {
  menu.value = menu.value === 'resize' ? '' : 'resize'
  resizeTo.width = doc.value.width
  resizeTo.height = doc.value.height
}
function resizeWidth(v) {
  resizeTo.width = Number(v)
  if (resizeTo.keep) resizeTo.height = Math.max(1, Math.round((resizeTo.width * doc.value.height) / doc.value.width))
}
function resizeHeight(v) {
  resizeTo.height = Number(v)
  if (resizeTo.keep) resizeTo.width = Math.max(1, Math.round((resizeTo.height * doc.value.width) / doc.value.height))
}
function applyResize() {
  menu.value = ''
  doResize(resizeTo.width, resizeTo.height)
}

// --- the companion's hand -----------------------------------------------------------------------
const chat = ref([])
const words = ref('')
const asking = ref(false)
const chatLog = ref(null)
const pending = ref([])
let pollTimer = null

function chatLine(who, text) {
  chat.value = [...chat.value.slice(-40), { id: `${Date.now()}-${chat.value.length}`, who, text }]
  nextTick(() => chatLog.value?.scrollTo({ top: chatLog.value.scrollHeight }))
}

async function send() {
  const text = words.value.trim()
  if (!text || asking.value) return
  if (!doc.value) return say('Open a picture first.')
  words.value = ''
  chatLine('me', text)
  asking.value = true
  try {
    const d = doc.value
    const picture = pngOf(shrink(flatten(d, pixels), 1568))
    const earlier = chat.value.slice(-7, -1).filter((m) => m.who !== 'note').map((m) => `${m.who === 'me' ? 'Member' : 'Companion'}: ${m.text}`)
    const answer = await photoAsk(text, picture, { ...docWithoutPixels(d), selection: selectionBox(d.selection) }, earlier)
    const ops = cleanOps(answer.ops, doc.value)
    chatLine('companion', answer.say || (ops.length ? 'Done.' : 'I could not find anything to change.'))
    await applyOps(ops)
  } catch (e) {
    chatLine('note', String(e?.message ?? e))
  } finally {
    asking.value = false
  }
}

async function applyOps(ops) {
  for (const raw of ops) {
    // checked again on the document as it is now (a crop before it changes the edges)
    const op = cleanOp(raw, doc.value)
    if (!op) continue
    const from = `${props.companion}: `
    switch (op.op) {
      case 'adjust':
        doAdjust(op.values, op.mode, from)
        break
      case 'crop':
        doCrop({ x: op.x, y: op.y, w: op.w, h: op.h }, from)
        break
      case 'rotate':
        doRotate(op.turns, from)
        break
      case 'flip':
        doFlip(op.axis, from)
        break
      case 'resize':
        doResize(op.width, op.height, from)
        break
      case 'text':
        addText(op.text, op.x, op.y, op.size, op.color, from)
        break
      case 'layer': {
        const { op: _kind, id, ...fields } = op
        const l = doc.value.layers.find((x) => x.id === id)
        changeFields(id, fields, `${from}${l?.name ?? 'Layer'} ${Object.keys(fields).join(', ')}`)
        break
      }
      case 'select':
        doSelect(op, from)
        break
      default:
        if (REDRAWS.includes(op.op)) await startRedraw(op)
    }
  }
}

async function startRedraw(op) {
  const d = doc.value
  try {
    const picture = pngOf(shrink(flatten(d, pixels), 2048))
    const region = op.op === 'erase' ? { x: op.x, y: op.y, w: op.w, h: op.h } : null
    const job = await photoRedraw(op.op, { prompt: op.prompt ?? null, region, picture, width: d.width, height: d.height })
    pending.value = [...pending.value, { id: job.id, kind: op.op }]
    chatLine('note', redrawLine(op.op, REDRAW_USD))
    watchRedraws()
  } catch (e) {
    chatLine('note', `${REDRAW_LABELS[op.op]} was not started: ${String(e?.message ?? e)}`)
  }
}

function watchRedraws() {
  if (pollTimer || !pending.value.length) return
  pollTimer = setInterval(checkRedraws, 2500)
}

async function checkRedraws() {
  if (!pending.value.length) {
    clearInterval(pollTimer)
    pollTimer = null
    return
  }
  let jobs = []
  try {
    jobs = await imageJobs()
  } catch {
    return
  }
  for (const wait of pending.value) {
    const job = jobs.find((j) => j.id === wait.id)
    if (!job) {
      pending.value = pending.value.filter((p) => p.id !== wait.id)
      chatLine('note', `${REDRAW_LABELS[wait.kind]} is no longer on the picture queue.`)
      continue
    }
    if (job.phase === 'completed') {
      pending.value = pending.value.filter((p) => p.id !== wait.id)
      try {
        await landRedraw(wait.kind, await photoResult(wait.id))
      } catch (e) {
        chatLine('note', `${REDRAW_LABELS[wait.kind]}: ${String(e?.message ?? e)}`)
      }
      imageDismiss(wait.id).catch(() => {})
    } else if (job.phase === 'prompt-saved') {
      pending.value = pending.value.filter((p) => p.id !== wait.id)
      // a refusal is one plain line
      chatLine('note', `${REDRAW_LABELS[wait.kind]} was not drawn. ${String(job.message ?? '').split('\n')[0]}`)
    }
  }
}

/** A redraw's picture on a new layer above the originals, which are hidden, never lost. */
async function landRedraw(kind, url) {
  if (!doc.value) return
  const d = doc.value
  const got = await loadCanvas(url)
  const c = scaleCanvas(got, d.width / got.width, d.height / got.height)
  const layer = imageLayer(REDRAW_LABELS[kind])
  const shown = d.layers.filter((l) => l.visible).map((l) => l.id)
  const hidden = { ...d, layers: d.layers.map((l) => ({ ...l, visible: false })) }
  commit(insertLayer(hidden, layer), { kind: 'layer-new', layerId: layer.id, shown, label: REDRAW_LABELS[kind] }, { ...pixels, [layer.id]: c })
  activeId.value = layer.id
  chatLine('companion', `${REDRAW_LABELS[kind]} is done: a new layer on top, with the originals hidden beneath it.`)
}

// --- the room's life ----------------------------------------------------------------------------
let stopDrop = null
let resize = null
onMounted(async () => {
  window.addEventListener('keydown', keyDown)
  window.addEventListener('keyup', keyUp)
  resize = new ResizeObserver(() => askFrame())
  if (viewport.value) resize.observe(viewport.value)
  try {
    stopDrop = await getCurrentWebview().onDragDropEvent(onDragDrop)
  } catch {
    // a browser without Tauri (a dev check): no drop
  }
  photoRecent().then((r) => (recent.value = r)).catch(() => {})
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', keyDown)
  window.removeEventListener('keyup', keyUp)
  resize?.disconnect()
  stopDrop?.()
  clearInterval(pollTimer)
  clearTimeout(thumbTimer)
})
watch(
  () => props.visible,
  (on) => {
    if (on) {
      markDirty()
      if (!doc.value) photoRecent().then((r) => (recent.value = r)).catch(() => {})
    } else {
      spaceDown = false
      dropping.value = false
    }
  },
)
watch(tool, () => {
  if (tool.value !== 'crop') cropRect.value = null
  askFrame()
})
watch(
  () => [opts.size, activeId.value],
  () => askFrame(),
)
</script>

<template>
  <section class="photo" :class="{ dropping }" aria-label="Photo">
    <header class="bar">
      <h2 class="title">PHOTO</h2>
      <div class="files" role="toolbar" aria-label="File">
        <button type="button" title="Open a PNG, JPG or WebP picture (Ctrl+O)" @click="openPicture">Open</button>
        <div class="pop-wrap">
          <button type="button" :aria-expanded="menu === 'recent'" @click="showRecent">Open recent</button>
          <div v-if="menu === 'recent'" class="pop recent" role="menu">
            <p v-if="!recent.length" class="quiet">Nothing saved yet.</p>
            <button v-for="r in recent" :key="r.name" type="button" class="recent-item" role="menuitem" @click="openRecent(r)">
              <img v-if="r.thumb" :src="r.thumb" alt="" />
              <span>{{ r.name }}</span>
            </button>
          </div>
        </div>
        <input v-model="name" class="name" type="text" maxlength="60" placeholder="Name" aria-label="The picture's name" :disabled="!doc" />
        <button type="button" :disabled="!doc || busy" title="Save as a project in media-generation/photo (Ctrl+S)" @click="save">Save{{ dirty ? ' *' : '' }}</button>
        <div class="pop-wrap">
          <button type="button" :disabled="!doc" :aria-expanded="menu === 'export'" @click="menu = menu === 'export' ? '' : 'export'">Export</button>
          <div v-if="menu === 'export'" class="pop" role="dialog" aria-label="Export">
            <label><input v-model="exportKind" type="radio" value="png" /> PNG</label>
            <label><input v-model="exportKind" type="radio" value="jpg" /> JPG</label>
            <label v-if="exportKind === 'jpg'" class="row">Quality <input v-model.number="exportQuality" type="range" min="10" max="100" /> {{ exportQuality }}</label>
            <button type="button" class="primary" :disabled="busy" @click="exportPicture">Export…</button>
          </div>
        </div>
      </div>
      <p class="said">Version 1 is basic: no RAW, PSD or CMYK. Redrawing edits use your image credit; everything else is free and instant.</p>
    </header>

    <div class="work">
      <nav class="tools" aria-label="Tools">
        <button v-for="t in TOOLS" :key="t.id" type="button" class="tool" :class="{ on: tool === t.id }" :title="`${t.label} (${t.key})`" :aria-label="t.label" :aria-pressed="tool === t.id" @click="tool = t.id">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path :d="t.d" /></svg>
        </button>
        <label class="swatch" title="Colour">
          <input v-model="opts.color" type="color" aria-label="Colour" />
        </label>
      </nav>

      <div class="stage">
        <div class="options" role="toolbar" aria-label="Tool options">
          <template v-if="['brush', 'eraser', 'clone'].includes(tool)">
            <label>Size <input v-model.number="opts.size" type="range" min="1" max="500" /> {{ opts.size }}</label>
            <label>Hardness <input v-model.number="opts.hardness" type="range" min="0" max="100" /> {{ opts.hardness }}</label>
            <label>Opacity <input v-model.number="opts.opacity" type="range" min="1" max="100" /> {{ opts.opacity }}%</label>
            <span v-if="tool === 'clone'" class="quiet">Alt+click the source, then paint.</span>
          </template>
          <template v-else-if="tool === 'wand'">
            <label>Tolerance <input v-model.number="opts.tolerance" type="range" min="0" max="255" /> {{ opts.tolerance }}</label>
            <label><input v-model="opts.contiguous" type="checkbox" /> Joined only</label>
          </template>
          <template v-else-if="tool === 'crop'">
            <span class="quiet">Drag a box, then Enter. Esc cancels.</span>
            <button type="button" :disabled="!cropRect && !doc?.selection" @click="applyCrop">Crop</button>
          </template>
          <template v-else-if="tool === 'text'">
            <label>Size <input v-model.number="opts.textSize" type="number" min="6" max="400" /></label>
            <span class="quiet">Click to add text, or click a text to choose it.</span>
          </template>
          <span v-else-if="tool === 'move'" class="quiet">Drag to move the chosen layer. Space+drag pans, Ctrl+wheel zooms.</span>
          <span v-else-if="tool === 'eyedropper'" class="quiet">Click the photo to pick its colour.</span>
          <span v-else class="quiet">Drag on the photo. Ctrl+A selects all, Ctrl+D deselects.</span>
          <span class="grow"></span>
          <template v-if="doc">
            <button type="button" title="Rotate left" @click="doRotate(3)">⟲</button>
            <button type="button" title="Rotate right" @click="doRotate(1)">⟳</button>
            <button type="button" title="Flip across" @click="doFlip('h')">⇋</button>
            <button type="button" title="Flip upside down" @click="doFlip('v')">⇵</button>
            <div class="pop-wrap">
              <button type="button" title="Resize" :aria-expanded="menu === 'resize'" @click="openResize">Resize</button>
              <div v-if="menu === 'resize'" class="pop right" role="dialog" aria-label="Resize">
                <label class="row">Width <input :value="resizeTo.width" type="number" min="1" :max="MAX_SIDE" @input="resizeWidth($event.target.value)" /></label>
                <label class="row">Height <input :value="resizeTo.height" type="number" min="1" :max="MAX_SIDE" @input="resizeHeight($event.target.value)" /></label>
                <label><input v-model="resizeTo.keep" type="checkbox" /> Keep the shape</label>
                <button type="button" class="primary" @click="applyResize">Resize</button>
              </div>
            </div>
            <button type="button" title="Fit to the window" @click="fit">{{ zoomLine }}</button>
          </template>
        </div>

        <div ref="viewport" class="viewport">
          <canvas
            ref="viewCanvas"
            :class="['tool-' + tool]"
            @pointerdown="pointerDown"
            @pointermove="pointerMove"
            @pointerup="pointerUp"
            @pointercancel="pointerUp"
            @pointerleave="leaveView"
            @wheel="wheel"
          ></canvas>
          <div v-if="!doc" class="empty">
            <p>Open a picture, or drop a PNG, JPG or WebP here.</p>
            <button type="button" class="primary" @click="openPicture">Open a picture</button>
            <div v-if="recent.length" class="recent-grid">
              <button v-for="r in recent.slice(0, 6)" :key="r.name" type="button" class="recent-card" @click="openRecent(r)">
                <img v-if="r.thumb" :src="r.thumb" alt="" />
                <span>{{ r.name }}</span>
              </button>
            </div>
          </div>
        </div>

        <div class="pills" role="list" aria-label="Undo, one edit at a time">
          <span v-if="!history.length" class="quiet">Each edit becomes a pill here; press one to undo that edit alone.</span>
          <button v-for="p in history" :key="p.id" type="button" role="listitem" class="pill" :title="`Undo: ${p.label}`" @click="undoOne(p.id)">
            {{ p.label }} <span aria-hidden="true">↶</span>
          </button>
        </div>
      </div>

      <aside class="side">
        <section class="panel" aria-label="Layers">
          <header class="panel-head">
            <h3>LAYERS</h3>
            <button type="button" :disabled="!doc" title="New layer" @click="newLayer">+ New</button>
          </header>
          <ul class="layers">
            <li v-for="l in layersTopFirst" :key="l.id" :class="{ on: l.id === activeId }" @click="activeId = l.id">
              <button type="button" class="eye" :title="l.visible ? 'Hide' : 'Show'" :aria-label="l.visible ? `Hide ${l.name}` : `Show ${l.name}`" @click.stop="changeFields(l.id, { visible: !l.visible }, `${l.visible ? 'Hide' : 'Show'} ${l.name}`)">
                {{ l.visible ? '◉' : '○' }}
              </button>
              <span class="thumb">
                <img v-if="thumbs[l.id]" :src="thumbs[l.id]" alt="" />
                <b v-else-if="l.kind === 'text'">T</b>
              </span>
              <input v-if="renaming === l.id" :id="`rename-${l.id}`" v-model="renameText" class="rename" maxlength="60" @click.stop @keydown.enter="finishRename(l)" @keydown.esc="renaming = null" @blur="finishRename(l)" />
              <span v-else class="layer-name" :title="'Double-click to rename'" @dblclick.stop="startRename(l)">{{ l.name }}</span>
              <span class="layer-tools">
                <button type="button" title="Up" @click.stop="moveLayerBy(l, 1)">▲</button>
                <button type="button" title="Down" @click.stop="moveLayerBy(l, -1)">▼</button>
                <button type="button" title="Duplicate" @click.stop="duplicateLayer(l)">⧉</button>
                <button type="button" title="Delete" @click.stop="deleteLayer(l)">✕</button>
              </span>
            </li>
          </ul>
          <div v-if="active" class="layer-props">
            <label class="row">Opacity <input :value="active.opacity" type="range" min="0" max="100" @input="opacityInput($event.target.value)" @change="opacityDone" /> {{ active.opacity }}%</label>
            <label class="row">Blend
              <GlassSelect :model-value="active.blend" :options="BLENDS.map((b) => ({ value: b, label: b }))" aria-label="Blend" @update:model-value="(b) => changeFields(active.id, { blend: b }, `${active.name} ${b}`)" />
            </label>
            <template v-if="active.kind === 'text'">
              <textarea id="photo-text-words" :value="active.text.value" rows="2" maxlength="500" aria-label="The text" @input="textInput({ value: $event.target.value })" @change="textDone" @blur="textDone"></textarea>
              <label class="row">Size <input :value="active.text.size" type="number" min="6" max="400" @input="textInput({ size: Math.min(400, Math.max(6, Number($event.target.value) || 6)) })" @change="textDone" /></label>
              <label class="row">Colour <input :value="active.text.color" type="color" @input="textInput({ color: $event.target.value })" @change="textDone" /></label>
            </template>
          </div>
        </section>

        <section class="panel" aria-label="Adjust">
          <header class="panel-head">
            <h3>ADJUST</h3>
            <button type="button" :disabled="!doc" @click="resetAdjust">Reset</button>
          </header>
          <label v-for="k in ADJUST_KEYS" :key="k" class="slider">
            <span>{{ ADJUST_LABELS[k] }}</span>
            <input :value="doc?.adjust[k] ?? 0" type="range" min="-100" max="100" :disabled="!doc" @input="sliderInput(k, $event.target.value)" @change="sliderDone(k)" @dblclick="doc && doAdjust({ [k]: 0 }, 'set')" />
            <output>{{ doc?.adjust[k] ?? 0 }}</output>
          </label>
        </section>
      </aside>
    </div>

    <p v-if="problem" class="problem" role="alert">{{ problem }}</p>
    <p v-else-if="notice" class="notice" role="status">{{ notice }}</p>

    <footer class="chat" aria-label="Chat with your companion">
      <div v-if="chat.length" ref="chatLog" class="log">
        <p v-for="m in chat" :key="m.id" :class="m.who"><b v-if="m.who === 'companion'">{{ companion }}:</b> {{ m.text }}</p>
      </div>
      <form class="ask" @submit.prevent="send">
        <input v-model="words" type="text" maxlength="2000" :placeholder="doc ? `Ask ${companion} to change the photo: warmer, crop to the face, remove the background...` : 'Open a picture first'" :disabled="asking" aria-label="What to change" />
        <span v-if="pending.length" class="quiet">Redrawing…</span>
        <button type="submit" class="primary" :disabled="asking || !words.trim() || !doc">{{ asking ? 'Thinking…' : 'Send' }}</button>
      </form>
    </footer>
  </section>
</template>

<style scoped>
.photo {
  display: grid;
  grid-template-rows: auto minmax(0, 1fr) auto auto;
  gap: 8px;
  height: 100%;
  padding: 10px 14px;
  box-sizing: border-box;
  min-height: 0;
}
.photo.dropping .viewport {
  outline: 2px dashed var(--accent, #b7a6ff);
  outline-offset: -6px;
}
.bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 16px;
}
.title {
  margin: 0;
  letter-spacing: 0.45em;
  font-weight: 500;
}
.files {
  display: flex;
  align-items: center;
  gap: 6px;
  flex-wrap: wrap;
}
.name {
  width: 180px;
}
.said {
  margin: 0;
  flex-basis: 100%;
  font-size: 0.78rem;
  opacity: 0.7;
}
.pop-wrap {
  position: relative;
}
.pop {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  z-index: 20;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 200px;
  padding: 10px;
  border-radius: 12px;
  border: 1px solid color-mix(in srgb, var(--accent, #b7a6ff) 45%, transparent);
  background: var(--panel-background, rgba(24, 18, 52, 0.96));
  backdrop-filter: blur(10px);
  box-shadow: 0 10px 30px rgba(0, 0, 0, 0.35);
}
.pop.right {
  left: auto;
  right: 0;
}
.pop.recent {
  max-height: 340px;
  overflow: auto;
  min-width: 240px;
}
.recent-item {
  display: flex;
  align-items: center;
  gap: 8px;
  text-align: left;
}
.recent-item img {
  width: 40px;
  height: 40px;
  object-fit: cover;
  border-radius: 6px;
}
.row {
  display: flex;
  align-items: center;
  gap: 8px;
}
.row input[type='number'] {
  width: 80px;
}
.work {
  display: grid;
  grid-template-columns: 48px minmax(0, 1fr) 280px;
  gap: 10px;
  min-height: 0;
}
.tools,
.stage,
.panel,
.chat {
  border: 1px solid color-mix(in srgb, var(--accent, #b7a6ff) 40%, transparent);
  border-radius: 14px;
  background: var(--panel-background, rgba(24, 18, 52, 0.72));
  backdrop-filter: blur(8px);
}
.tools {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  padding: 6px 0;
  overflow-y: auto;
}
.tool {
  display: grid;
  place-items: center;
  width: 36px;
  height: 36px;
  padding: 0;
  border-radius: 9px;
}
.tool svg {
  width: 20px;
  height: 20px;
}
.tool.on {
  background: color-mix(in srgb, var(--accent, #b7a6ff) 35%, transparent);
  outline: 1px solid var(--accent, #b7a6ff);
}
.swatch {
  margin-top: 6px;
}
.swatch input {
  width: 32px;
  height: 32px;
  padding: 0;
  border: none;
  background: none;
  cursor: pointer;
}
.stage {
  display: grid;
  grid-template-rows: auto minmax(0, 1fr) auto;
  min-height: 0;
  padding: 8px;
  gap: 6px;
}
.options {
  display: flex;
  align-items: center;
  flex-wrap: wrap;
  gap: 6px 14px;
  font-size: 0.85rem;
}
.options label {
  display: flex;
  align-items: center;
  gap: 6px;
}
.options input[type='range'] {
  width: 90px;
}
.options input[type='number'] {
  width: 64px;
}
.grow {
  flex: 1;
}
.viewport {
  position: relative;
  min-height: 200px;
  overflow: hidden;
  border-radius: 10px;
  background: rgba(0, 0, 0, 0.28);
}
.viewport canvas {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
  touch-action: none;
}
.tool-move {
  cursor: move;
}
.tool-marquee,
.tool-lasso,
.tool-crop,
.tool-wand,
.tool-eyedropper {
  cursor: crosshair;
}
.tool-brush,
.tool-eraser,
.tool-clone {
  cursor: none;
}
.tool-text {
  cursor: text;
}
.empty {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  text-align: center;
  padding: 20px;
}
.recent-grid {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 10px;
  margin-top: 8px;
}
.recent-card {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  width: 110px;
  padding: 6px;
}
.recent-card img {
  width: 96px;
  height: 72px;
  object-fit: cover;
  border-radius: 6px;
}
.pills {
  display: flex;
  gap: 6px;
  overflow-x: auto;
  padding-bottom: 2px;
  min-height: 30px;
  align-items: center;
}
.pill {
  flex: none;
  padding: 3px 10px;
  border-radius: 999px;
  font-size: 0.78rem;
  white-space: nowrap;
}
.side {
  display: flex;
  flex-direction: column;
  gap: 10px;
  overflow-y: auto;
  min-height: 0;
}
.panel {
  padding: 8px 10px;
}
.panel-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-bottom: 6px;
}
.panel-head h3 {
  margin: 0;
  font-size: 0.8rem;
  letter-spacing: 0.25em;
  font-weight: 600;
}
.layers {
  list-style: none;
  margin: 0;
  padding: 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
  max-height: 260px;
  overflow-y: auto;
}
.layers li {
  display: grid;
  grid-template-columns: 22px 34px minmax(0, 1fr) auto;
  align-items: center;
  gap: 6px;
  padding: 3px 4px;
  border-radius: 8px;
  cursor: pointer;
}
.layers li.on {
  background: color-mix(in srgb, var(--accent, #b7a6ff) 25%, transparent);
}
.eye {
  padding: 0;
  width: 22px;
  height: 22px;
  border: none;
  background: none;
}
.thumb {
  display: grid;
  place-items: center;
  width: 34px;
  height: 26px;
  border-radius: 4px;
  overflow: hidden;
  background: repeating-conic-gradient(#c9c9d6 0% 25%, #e6e6ee 0% 50%) 50% / 8px 8px;
  color: #222;
}
.thumb img {
  max-width: 100%;
  max-height: 100%;
}
.layer-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 0.85rem;
}
.rename {
  width: 100%;
  min-width: 0;
}
.layer-tools {
  display: flex;
  gap: 2px;
}
.layer-tools button {
  padding: 0 4px;
  min-width: 0;
  font-size: 0.72rem;
  border: none;
  background: none;
  opacity: 0.75;
}
.layer-tools button:hover {
  opacity: 1;
}
.layer-props {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 8px;
  font-size: 0.85rem;
}
.layer-props textarea {
  width: 100%;
  box-sizing: border-box;
  resize: vertical;
}
.slider {
  display: grid;
  grid-template-columns: 82px minmax(0, 1fr) 34px;
  align-items: center;
  gap: 6px;
  font-size: 0.82rem;
  margin: 3px 0;
}
.slider output {
  text-align: right;
  opacity: 0.8;
}
.problem {
  margin: 0;
  color: var(--danger, #ff9b9b);
}
.notice {
  margin: 0;
  opacity: 0.85;
}
.quiet {
  opacity: 0.65;
  font-size: 0.82rem;
}
.chat {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 8px 10px;
}
.log {
  max-height: 110px;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
  gap: 3px;
  font-size: 0.86rem;
}
.log p {
  margin: 0;
}
.log .me {
  opacity: 0.8;
  text-align: right;
}
.log .note {
  opacity: 0.7;
  font-style: italic;
}
.ask {
  display: flex;
  gap: 8px;
  align-items: center;
}
.ask input {
  flex: 1;
  min-width: 0;
}
@media (max-width: 900px) {
  .work {
    grid-template-columns: 44px minmax(0, 1fr);
  }
  .side {
    grid-column: 1 / -1;
    flex-direction: row;
    max-height: 260px;
  }
  .panel {
    flex: 1;
  }
}
</style>
