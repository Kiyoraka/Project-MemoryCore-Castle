<script setup>
import GlassSelect from '../components/GlassSelect.vue'
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import {
  avatarGet,
  imageDismiss,
  imageJobs,
  onPhotoRequest,
  photoActionsLoad,
  photoActionsSave,
  photoAsk,
  photoBatchList,
  photoBatchWrite,
  photoDoorDone,
  photoExport,
  photoExportMany,
  photoLoad,
  photoOpenDialog,
  photoPickFolder,
  photoRead,
  photoReadBytes,
  photoRecent,
  photoRedraw,
  photoRequestTake,
  photoResult,
  photoSave,
  photoSaveBack,
} from '../api/castle.js'
import { pictureCost } from '../components/appearance.js'
import { doorDone, formatForPath, nativeOnly, readRequest, takeOnce } from './photo/photoDoor.js'
import { applyAdjust, cleanName, emptyAdjust, hexToRgba, isAdjustEmpty, maskBounds, newDoc, renameLayer, restorePixels, setLayer } from './photo/engine.js'
import { addNoise, colorRange, contentAwareFill, drawShape, gaussianBlur, grabCut, invertMask, median, refineMask, skyMask, unsharp, vignette } from './photo/engine2.js'
import * as engine3 from './photo/engine3.js'
import { psdToLayers, readPsd } from './photo/psd.js'
import {
  ADJUSTMENT_NAMES,
  ADJUSTMENT_TYPES,
  STYLE_KEYS,
  StepHistory,
  addAdjustmentLayer,
  addArtboard,
  addFullLayer,
  addGuide,
  blendIds,
  cleanActions,
  deleteLayer3,
  duplicateLayer3,
  fillPixels,
  groupLayers,
  isGroup,
  keepRatio,
  localMask,
  moveGuide,
  normaliseDoc3,
  placeLayer,
  removeGuide,
  removePath,
  renamePath,
  setAdjustment,
  setGroupFolded,
  setLookOptions,
  setMask,
  setStyles,
  stripForSave,
  toggleClip,
  toggleStyle,
  ungroupLayers,
  uniqueActionName,
  whiteMask,
} from './photo/viewHelpers.js'
import * as ops from './photo/ops.js'
import PhotoCanvas from './photo/PhotoCanvas.vue'
import ToolColumn from './photo/ToolColumn.vue'
import MenuBar from './photo/MenuBar.vue'
import Dock from './photo/Dock.vue'
import LayersPanel from './photo/LayersPanel.vue'
import ChannelsPanel from './photo/ChannelsPanel.vue'
import PathsPanel from './photo/PathsPanel.vue'
import PropertiesPanel from './photo/PropertiesPanel.vue'
import AdjustmentsPanel from './photo/AdjustmentsPanel.vue'
import StylesPanel from './photo/StylesPanel.vue'
import SwatchesPanel from './photo/SwatchesPanel.vue'
import ActionsPanel from './photo/ActionsPanel.vue'
import HistoryPanel from './photo/HistoryPanel.vue'
import NavigatorPanel from './photo/NavigatorPanel.vue'
import LayerStyleDialog from './photo/LayerStyleDialog.vue'
import ExportAsDialog from './photo/ExportAsDialog.vue'
import Rulers from './photo/Rulers.vue'
import SelectMaskWorkspace from './photo/SelectMaskWorkspace.vue'
import LiquifyWorkspace from './photo/LiquifyWorkspace.vue'
import UndoPills from './photo/UndoPills.vue'
import PhotoChat from './photo/PhotoChat.vue'

// PHOTO v3 (the Keep's PHOTO, brought whole): v2's menus, tools, masks, adjustment layers, PROPERTIES and
// HISTORY - now with groups and clipping, layer styles, 27 blend modes, ~22 adjustments and the new filters,
// rulers, guides, grid and snapping, artboards with Export As, actions with batch, PSD import, quick mask,
// Select and Mask and Liquify, in a tabbed dock. And the companion in the chat bar, its hand gliding over the
// canvas, every change of its own a pill the member can undo alone. A picture handed in from DEV's FILES or a
// project's terminal (photo_door.rs) waits in the core until this room takes it.
const props = defineProps({
  companion: { type: String, default: 'Your companion' },
  cli: { type: String, default: 'claude' },
  visible: { type: Boolean, default: true },
})

const SHORTCUTS = { v: 'move', m: 'rect', l: 'lasso', w: 'wand', c: 'crop', j: 'heal', b: 'brush', s: 'clone', e: 'eraser', g: 'gradient', u: 'shape', p: 'pen', t: 'text', i: 'eyedropper', h: 'hand', z: 'zoom', o: 'dodge' }
// Shift+key steps through the tools that share a key, as in Photoshop
const CYCLES = { o: ['dodge', 'burn', 'sponge'], j: ['heal', 'spotHeal', 'patch'], w: ['wand', 'quickSelect', 'objectSelect'], r: ['blurBrush', 'sharpenBrush', 'smudge'] }
const PICTURE = /\.(png|jpe?g|webp|gif|bmp|psd)$/i
const BLENDS = blendIds(engine3.BLEND_MODES)
setLookOptions(engine3.LOOKS)

// the tools v3 adds, beside the v2 column
const EXTRA_TOOLS = [
  { id: 'quickSelect', label: 'Quick select (Shift+W)', path: 'M5 19c2-2 3-5 6-7s6-2 8-5M15 4l1 2 2 1-2 1-1 2-1-2-2-1 2-1Z' },
  { id: 'objectSelect', label: 'Object select (Shift+W)', path: 'M4 7V4h3m10 0h3v3m0 10v3h-3M7 20H4v-3M9 9h6v6H9Z' },
  { id: 'spotHeal', label: 'Spot healing brush (Shift+J)', path: 'M10.6 3.6a3 3 0 0 1 4.2 0l5.6 5.6a3 3 0 0 1 0 4.2l-7 7a3 3 0 0 1-4.2 0l-5.6-5.6a3 3 0 0 1 0-4.2l7-7ZM12 9v6m-3-3h6' },
  { id: 'patch', label: 'Patch (Shift+J)', path: 'M5 6h8v6H5ZM11 12l3 3h5v5h-6v-5' },
  { id: 'dodge', label: 'Dodge (O)', path: 'M9 15a5 5 0 1 0 0-10 5 5 0 0 0 0 10Zm3.5-1.5L20 21' },
  { id: 'burn', label: 'Burn (Shift+O)', path: 'M7 21c0-4 5-5 5-9m0 0c3 2 4 5 3 9M9 4c0 3 3 4 3 8' },
  { id: 'sponge', label: 'Sponge (Shift+O)', path: 'M5 8a3 3 0 0 1 3-3h8a3 3 0 0 1 3 3v8a3 3 0 0 1-3 3H8a3 3 0 0 1-3-3ZM9 9h.01M14 10h.01M10 14h.01M15 15h.01' },
  { id: 'smudge', label: 'Smudge (Shift+R)', path: 'M8 21v-6M8 15c0-2-2-3-2-6a2 2 0 0 1 4 0v4m0-3a2 2 0 0 1 4 0v4m0-3a2 2 0 0 1 4 0v5a6 6 0 0 1-6 6' },
  { id: 'blurBrush', label: 'Blur brush (R)', path: 'M12 3s6 6.4 6 11a6 6 0 0 1-12 0c0-4.6 6-11 6-11Z' },
  { id: 'sharpenBrush', label: 'Sharpen brush (Shift+R)', path: 'M12 3 18 19H6L12 3Z' },
]

const DOCK = [
  { id: 'layers-group', grow: 3, tabs: [{ id: 'layers', label: 'LAYERS' }, { id: 'channels', label: 'CHANNELS' }, { id: 'paths', label: 'PATHS' }] },
  { id: 'props-group', grow: 3, tabs: [{ id: 'properties', label: 'PROPERTIES' }] },
  { id: 'adjust-group', grow: 2, tabs: [{ id: 'adjustments', label: 'ADJUSTMENTS' }, { id: 'styles', label: 'STYLES' }, { id: 'swatches', label: 'SWATCHES' }] },
  { id: 'actions-group', grow: 2, tabs: [{ id: 'actions', label: 'ACTIONS' }, { id: 'history', label: 'HISTORY' }, { id: 'navigator', label: 'NAVIGATOR' }] },
]
const PANELS = DOCK.flatMap((g) => g.tabs).map((t) => ({ id: t.id, label: t.label.charAt(0) + t.label.slice(1).toLowerCase() }))
const ADJ_ORDER = ['brightnessContrast', 'levels', 'curves', 'exposure', 'vibrance', 'hsl', 'colorBalance', 'blackWhite', 'photoFilter', 'channelMixer', 'colorLookup', 'invert', 'posterize', 'threshold', 'gradientMap', 'selectiveColor', 'shadowsHighlights', 'adjust']
const ADJ_KEYS = { curves: 'Ctrl+M', levels: 'Ctrl+L', hsl: 'Ctrl+U' }
const ADJ_MENU = [...ADJ_ORDER.filter((t) => ADJUSTMENT_TYPES.includes(t)), ...ADJUSTMENT_TYPES.filter((t) => !ADJ_ORDER.includes(t))].map((type) => ({ type, label: ADJUSTMENT_NAMES[type], keys: ADJ_KEYS[type] }))
const PRESETS = (Array.isArray(engine3.artboardPresets) ? engine3.artboardPresets : []).map((p) => ({ id: p.id, label: p.name, w: p.width, h: p.height }))

function loadView() {
  try {
    return { rulers: true, guides: true, lock: false, snap: true, hidden: [], ...JSON.parse(localStorage.getItem('photo.view') || '{}') }
  } catch {
    return { rulers: true, guides: true, lock: false, snap: true, hidden: [] }
  }
}

const doc = ref(null)
const selectedId = ref('')
const name = ref('Untitled')
const tool = ref('move')
const primary = ref('#ffffff')
const secondary = ref('#000000')
const brush = reactive({ size: 24, hardness: 0.7, opacity: 1, preset: 'soft', flow: 1, build: false, texture: 0, pressureSize: false, pressureOpacity: false, strength: 0.5, range: 'midtones', mode: 'saturate' })
const tolerance = ref(32)
const gradient = ref({ kind: 'linear', stops: [{ at: 0, color: '#ffffff' }, { at: 1, color: '#000000' }] })
const shape = ref({ kind: 'rect', fill: '#ffffff', fillOn: true, stroke: '#000000', strokeOn: false, width: 4 })
const editMask = ref(false)
const viewMask = ref('')
const transformT = ref(null)
const histogram = ref(null)
const dirty = ref(false)
const pills = ref([])
const sayLines = ref([])
const asking = ref(false)
const problem = ref('')
const portrait = ref(null)
const recent = ref([])
const showRecent = ref(false)
const busy = ref('')
const notice = ref('')
const dropping = ref(false)
const armed = ref('')
const saveBackPath = ref('')
const dialog = ref(null) // { title, fields, values, ok, apply(values), preview?(values) -> { id, fn } }
const canvas = ref(null)
const dock = ref(null)
const room = ref(null)
const history = new StepHistory(40)
const historyTick = ref(0)
// v3
const viewPrefs = reactive(loadView())
const view = ref(null) // { stageW, stageH, scale, x, y } from the canvas
const channel = ref('')
const quickMask = ref(false)
const activePathId = ref('')
const thumb = ref(null) // { url, pixels }
const styleDialog = ref(null) // { id, before, start }
const exportOpen = ref(false)
const workspace = ref(null) // { kind: 'mask', pixels, mask } | { kind: 'liquify', id, pixels, origin, name }
const actions = ref([])
const recording = ref('')
const recSteps = ref([])
const actionBusy = ref('')
let noticeTimer
let armTimer
let stopDrop
let stopRequest
let pillCount = 0
let previewTimer
let histTimer
let thumbTimer

watch(viewPrefs, () => {
  try {
    localStorage.setItem('photo.view', JSON.stringify(viewPrefs))
  } catch {
    // a private window keeps nothing
  }
})

const canUndo = computed(() => historyTick.value >= 0 && history.canUndo)
const canRedo = computed(() => historyTick.value >= 0 && history.canRedo)
const historyEntries = computed(() => (historyTick.value >= 0 ? history.entries() : []))
const historyIndex = computed(() => (historyTick.value >= 0 ? history.index : -1))
const selectedLayer = computed(() => doc.value?.layers.find((l) => l.id === selectedId.value) || null)
const selectedIndex = computed(() => doc.value?.layers.findIndex((l) => l.id === selectedId.value) ?? -1)
const canMergeDown = computed(() => {
  const i = selectedIndex.value
  if (i <= 0) return false
  const upper = doc.value.layers[i]
  const lower = doc.value.layers[i - 1]
  return !isGroup(upper) && !isGroup(lower) && lower.kind !== 'adjustment' && (upper.parent || '') === (lower.parent || '')
})
const saveBackName = computed(() => String(saveBackPath.value || '').replace(/^.*[\\/]/, ''))
const activePath = computed(() => doc.value?.paths?.find((p) => p.id === activePathId.value) || null)
const filterMenu = computed(() => Object.entries(FILTERS).map(([id, f]) => ({ id, label: f.title, group: f.group })))
const menuChecks = computed(() => ({
  rulers: viewPrefs.rulers,
  'guides-show': viewPrefs.guides,
  'guides-lock': viewPrefs.lock,
  grid: !!doc.value?.grid?.show,
  snap: viewPrefs.snap,
  'quick-mask': quickMask.value,
  ...Object.fromEntries(PANELS.map((p) => [`panel-${p.id}`, !viewPrefs.hidden.includes(p.id)])),
}))

function say(text) {
  notice.value = text
  clearTimeout(noticeTimer)
  noticeTimer = setTimeout(() => (notice.value = ''), 5000)
}

function fail(reason) {
  problem.value = String(reason?.message || reason)
}

// Unsaved work asks for a second press before it is thrown away.
function confirmed(key, words) {
  if (!dirty.value || armed.value === key) {
    armed.value = ''
    return true
  }
  armed.value = key
  clearTimeout(armTimer)
  armTimer = setTimeout(() => (armed.value = ''), 4000)
  say(words)
  return false
}

function keepSelection() {
  const layers = doc.value?.layers || []
  if (!layers.some((l) => l.id === selectedId.value)) selectedId.value = layers.at(-1)?.id ?? ''
  const l = layers.find((x) => x.id === selectedId.value)
  if (!l?.mask) editMask.value = false
  if (viewMask.value && !layers.some((x) => x.id === viewMask.value && x.mask)) viewMask.value = ''
  if (activePathId.value && !doc.value?.paths?.some((p) => p.id === activePathId.value)) activePathId.value = ''
}

// ---------- actions: recording ----------

// While he records, every step he or she takes that has an op is kept.
function recordStep(op) {
  if (!recording.value || !op || typeof op.op !== 'string') return
  const step = JSON.parse(JSON.stringify(op))
  recSteps.value = [...recSteps.value, step].slice(0, 500)
  actions.value = actions.value.map((a) => (a.name === recording.value ? { ...a, steps: recSteps.value } : a))
}

// The last recorded step changed again (a slider of the same adjustment let go once more).
function rerecordLast(match, op) {
  if (!recording.value) return
  const last = recSteps.value.at(-1)
  if (last && match(last)) {
    recSteps.value = [...recSteps.value.slice(0, -1), JSON.parse(JSON.stringify(op))]
    actions.value = actions.value.map((a) => (a.name === recording.value ? { ...a, steps: recSteps.value } : a))
  } else recordStep(op)
}

// Every finished edit: the new document, one history step with its label (hers marked); meta.op is the step
// as her op, kept while an action records.
function commit(next, label, meta = {}) {
  if (!next || next === doc.value) return
  doc.value = next
  history.push(next, { label: label || 'Edit', hers: !!meta.hers })
  historyTick.value++
  dirty.value = true
  keepSelection()
  if (meta.op && !meta.hers) recordStep(meta.op)
  return label
}

function startDoc(next, title) {
  doc.value = next
  history.reset(next, { label: 'Open' })
  historyTick.value++
  selectedId.value = next.layers.at(-1)?.id ?? ''
  name.value = cleanName(title)
  pills.value = []
  problem.value = ''
  dirty.value = false
  editMask.value = false
  viewMask.value = ''
  channel.value = ''
  activePathId.value = ''
  saveBackPath.value = ''
  if (quickMask.value) {
    quickMask.value = false
    canvas.value?.endQuickMask()
  }
  canvas.value?.cancelTransform()
  canvas.value?.resetView()
}

function travel(state) {
  if (!state) return
  canvas.value?.cancelTransform()
  doc.value = state
  historyTick.value++
  dirty.value = true
  keepSelection()
}

const undo = () => travel(history.undo())
const redo = () => travel(history.redo())
const jump = (i) => travel(history.jump(i))

// ---------- opening ----------

// Any picture the core read (png, jpeg or webp) as a PNG with its real size.
function toPng(src) {
  return new Promise((resolve, reject) => {
    const img = new Image()
    img.onload = () => {
      const c = document.createElement('canvas')
      c.width = img.naturalWidth
      c.height = img.naturalHeight
      c.getContext('2d').drawImage(img, 0, 0)
      resolve({ png: src.startsWith('data:image/png') ? src : c.toDataURL('image/png'), width: c.width, height: c.height })
    }
    img.onerror = () => reject(new Error('That picture could not be read.'))
    img.src = src
  })
}

const stem = (file) => String(file || 'Photo').replace(/^.*[\\/]/, '').replace(/\.[^.]+$/, '')

function docFromPicture(pic, title) {
  return normaliseDoc3(addFullLayer(newDoc(pic.width, pic.height), pic.png, title || 'Background').doc, BLENDS)
}

function bytesOf(b64) {
  const bin = atob(String(b64).replace(/^data:[^,]*,/, ''))
  const out = new Uint8Array(bin.length)
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i)
  return out
}

// A Photoshop file: its layers and groups (pictures; text and smart objects as they were drawn), or its merged
// picture when it has none. What could not come in is said plainly.
async function openPsd(path) {
  const psd = readPsd(bytesOf(await photoReadBytes(path)))
  const layers = []
  for (const l of psdToLayers(psd)) {
    if (l.kind === 'group') layers.push({ id: l.id, name: l.name, kind: 'group', parent: l.parent, collapsed: !!l.collapsed, opacity: l.opacity, blend: l.blend, visible: l.visible, clipped: l.clipped })
    else if (l.pixels?.width && l.pixels?.height) layers.push({ id: l.id, name: l.name, kind: 'image', parent: l.parent, png: canvas.value.pngFrom(l.pixels), x: l.x || 0, y: l.y || 0, opacity: l.opacity, blend: l.blend, visible: l.visible, clipped: l.clipped })
  }
  if (!layers.some((l) => l.kind === 'image') && psd.merged) layers.splice(0, layers.length, { id: 'psd-merged', name: 'Background', kind: 'image', png: canvas.value.pngFrom(psd.merged), x: 0, y: 0 })
  if (!layers.length) throw new Error('That PSD has no picture in it.')
  const title = stem(path)
  startDoc(normaliseDoc3({ width: psd.width, height: psd.height, layers }, BLENDS), title)
  if (psd.notes?.length) say(`PSD opened. ${psd.notes.join('; ')}.`)
}

async function openPath(path) {
  if (!confirmed(`open:${path}`, 'Unsaved changes - open it again to throw them away.')) return
  busy.value = 'open'
  try {
    if (/\.psd$/i.test(path)) return await openPsd(path)
    const got = await photoRead(path)
    const pic = await toPng(got.png)
    pic.width ||= got.width
    pic.height ||= got.height
    const title = stem(got.name || path)
    startDoc(docFromPicture(pic, title), title)
  } catch (reason) {
    fail(reason)
  } finally {
    busy.value = ''
  }
}

async function openPicked() {
  try {
    const path = await photoOpenDialog()
    if (path) await openPath(path)
  } catch (reason) {
    fail(reason)
  }
}

// A picture handed in from elsewhere in Castle; with saveBackPath a "Save back" button appears.
async function openExternal({ png, name: title, saveBackPath: back } = {}) {
  if (!png) return
  busy.value = 'open'
  try {
    const src = String(png).startsWith('data:') ? png : `data:image/png;base64,${png}`
    const pic = await toPng(src)
    const label = stem(title || back || 'Photo')
    startDoc(docFromPicture(pic, label), label)
    saveBackPath.value = back || ''
  } catch (reason) {
    fail(reason)
  } finally {
    busy.value = ''
  }
}

async function toggleRecent() {
  showRecent.value = !showRecent.value
  if (!showRecent.value) return
  try {
    recent.value = (await photoRecent()) ?? []
  } catch (reason) {
    recent.value = []
    say(String(reason))
  }
}

async function openRecent(entry) {
  if (!confirmed(`recent:${entry.name}`, 'Unsaved changes - choose it again to open anyway.')) return
  busy.value = 'open'
  try {
    const got = await photoLoad(entry.name)
    startDoc(restorePixels(normaliseDoc3(got?.doc, BLENDS), got?.layers), entry.name)
    showRecent.value = false
  } catch (reason) {
    say(String(reason))
  } finally {
    busy.value = ''
  }
}

function blankPng(w, h, fill = '') {
  const c = document.createElement('canvas')
  c.width = w
  c.height = h
  if (fill) {
    const ctx = c.getContext('2d')
    ctx.fillStyle = fill
    ctx.fillRect(0, 0, w, h)
  }
  return c.toDataURL('image/png')
}

function newDocument({ width, height, background }) {
  if (!confirmed('new', 'Unsaved changes - choose New again to throw them away.')) return
  const w = Math.max(1, Math.min(12000, Math.round(width)))
  const h = Math.max(1, Math.min(12000, Math.round(height)))
  const fill = background === 'white' ? '#ffffff' : background === 'black' ? '#000000' : ''
  startDoc(normaliseDoc3(addFullLayer(newDoc(w, h), blankPng(w, h, fill), 'Background').doc, BLENDS), 'Untitled')
}

// ---------- saving, export, save back ----------

async function save() {
  if (!doc.value || busy.value) return
  busy.value = 'save'
  try {
    const d = doc.value
    const layers = []
    for (const l of d.layers) if (l.kind === 'image') layers.push({ id: l.id, png: await canvas.value.layerPngBase64(l.id) })
    const answer = await photoSave({ name: cleanName(name.value), doc: stripForSave(d), layers })
    name.value = answer?.name || cleanName(name.value)
    dirty.value = false
    say(`Saved as "${name.value}".`)
  } catch (reason) {
    say(String(reason))
  } finally {
    busy.value = ''
  }
}

async function exportAs(format, quality = 90) {
  if (!doc.value) return
  busy.value = 'export'
  try {
    const image = await canvas.value.flattenBase64(true, { format, quality })
    const path = await photoExport({ image, format, quality })
    if (path) say(`Exported to ${path}`)
  } catch (reason) {
    say(String(reason))
  } finally {
    busy.value = ''
  }
}

// Save back to the project's file: encoded in the file's own format, the old file kept once as .bak
async function writeBack() {
  const format = formatForPath(saveBackPath.value)
  if (!format) throw new Error('That file cannot be saved back as a picture.')
  const image = await canvas.value.flattenBase64(true, { format, quality: 92 })
  await photoSaveBack(saveBackPath.value, image)
  dirty.value = false
}

async function saveBack() {
  if (!doc.value || !saveBackPath.value) return
  busy.value = 'save'
  try {
    await writeBack()
    say(`Saved back to ${saveBackName.value} (the old one is kept as .bak).`)
  } catch (reason) {
    fail(reason)
  } finally {
    busy.value = ''
  }
}

// ---------- Export As: artboards and sizes ----------

// The flattened picture (doc.adjust in) as a canvas.
async function flatCanvas(d) {
  let pixels = await canvas.value.composite(d)
  if (!isAdjustEmpty(d.adjust)) pixels = applyAdjust(pixels, d.adjust)
  const c = document.createElement('canvas')
  c.width = pixels.width
  c.height = pixels.height
  c.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(pixels.data), pixels.width, pixels.height), 0, 0)
  return c
}

// One part of the picture (rect) at w x h -> base64 in the format (jpeg on white).
function encodeRegion(flat, rect, w, h, format, quality = 92) {
  const c = document.createElement('canvas')
  c.width = Math.max(1, Math.round(w))
  c.height = Math.max(1, Math.round(h))
  const ctx = c.getContext('2d')
  if (format === 'jpeg') {
    ctx.fillStyle = '#ffffff'
    ctx.fillRect(0, 0, c.width, c.height)
  }
  ctx.imageSmoothingQuality = 'high'
  ctx.drawImage(flat, rect.x, rect.y, rect.w, rect.h, 0, 0, c.width, c.height)
  const type = format === 'jpeg' ? 'image/jpeg' : format === 'webp' ? 'image/webp' : 'image/png'
  return c.toDataURL(type, format === 'png' ? undefined : quality / 100).split(',')[1]
}

// Her export_as and his dialog: each artboard (or the whole canvas), at 1x or fitted into each size.
async function exportArtboards(d, sizes, { artboards = false, format = 'png', quality = 92 } = {}) {
  const flat = await flatCanvas(d)
  const sources = artboards && d.artboards?.length ? d.artboards.map((a) => ({ name: a.name, rect: { x: a.x, y: a.y, w: a.w, h: a.h } })) : [{ name: cleanName(name.value) || 'picture', rect: { x: 0, y: 0, w: d.width, h: d.height } }]
  const files = []
  for (const s of sources) {
    if (!sizes?.length) files.push({ name: s.name, image: encodeRegion(flat, s.rect, s.rect.w, s.rect.h, format, quality), format })
    else
      for (const z of sizes) {
        const fit = engine3.fitInto(s.rect.w, s.rect.h, z.w, z.h, 'contain')
        files.push({ name: `${s.name}-${z.name || `${z.w}x${z.h}`}`, image: encodeRegion(flat, s.rect, fit.width, fit.height, format, quality), format })
      }
  }
  return files
}

async function runExportAs(spec) {
  busy.value = 'export'
  try {
    const flat = await flatCanvas(doc.value)
    const files = spec.plan.map((p) => ({ name: p.name, image: encodeRegion(flat, p.rect, p.w, p.h, spec.format, spec.quality), format: spec.format }))
    const folder = await photoExportMany({ files })
    exportOpen.value = false
    if (folder) {
      say(`Exported ${files.length} file${files.length === 1 ? '' : 's'} to ${folder}`)
      recordStep({ op: 'export_as', format: spec.format, artboards: spec.boards.length > 0, ...(spec.sizes.length ? { sizes: spec.sizes.map((n) => ({ w: n, h: n, name: String(n) })) } : {}) })
    }
  } catch (reason) {
    fail(reason)
  } finally {
    busy.value = ''
  }
}

// ---------- layers ----------

function addLayer() {
  if (!doc.value) return
  const d = doc.value
  const made = addFullLayer(d, blankPng(d.width, d.height), 'Layer', selectedId.value)
  // a new layer inside a group when the chosen one is in it
  const sel = selectedLayer.value
  const next = sel?.parent ? setLayer(made.doc, made.id, { parent: sel.parent }) : made.doc
  commit(next, 'New layer')
  selectedId.value = made.id
}

function onDuplicate(id) {
  const made = duplicateLayer3(doc.value, id)
  if (!made.id) return
  commit(made.doc, 'Layer duplicated')
  selectedId.value = made.id
}

function onRemove(id) {
  const next = deleteLayer3(doc.value, id)
  if (next === doc.value) return say('The last layer stays.')
  commit(next, isGroup(doc.value.layers.find((l) => l.id === id)) ? 'Group deleted' : 'Layer deleted')
}

const toggleLayer = (id) => {
  const l = doc.value.layers.find((x) => x.id === id)
  if (l) commit(setLayer(doc.value, id, { visible: !l.visible }), l.visible ? 'Layer hidden' : 'Layer shown', { op: { op: 'layer', name: l.name, set: { visible: !l.visible } } })
}

// Live while a slider moves; one history step when it is let go.
function onLayerSet(id, fields, final) {
  const next = setLayer(doc.value, id, fields)
  if (!final) return (doc.value = next)
  const l = doc.value.layers.find((x) => x.id === id)
  const op = 'blend' in fields ? { op: 'blend', layer: l?.name, mode: fields.blend } : { op: 'layer', name: l?.name, set: { opacity: fields.opacity } }
  commit(next, 'blend' in fields ? 'Blend mode' : 'Layer opacity', { op })
}

function onGroup(id = selectedId.value) {
  if (!doc.value) return
  const made = groupLayers(doc.value, id)
  const l = doc.value.layers.find((x) => x.id === id)
  commit(made.doc, 'Grouped', l ? { op: { op: 'group', layers: [l.name] } } : {})
  selectedId.value = made.id
}

function onUngroup(id = selectedId.value) {
  const g = doc.value?.layers.find((l) => l.id === id)
  if (!isGroup(g)) return
  commit(ungroupLayers(doc.value, id), 'Ungrouped', { op: { op: 'ungroup', group: g.name } })
}

function onClip(id = selectedId.value) {
  const l = doc.value?.layers.find((x) => x.id === id)
  if (!l) return
  if (!l.clipped && doc.value.layers.indexOf(l) === 0) return say('There is nothing below that layer to clip to.')
  commit(toggleClip(doc.value, id), l.clipped ? 'Released clipping mask' : 'Clipping mask', { op: { op: 'clip', layer: l.name, on: !l.clipped } })
}

function onFold(id, collapsed) {
  // a fold is how he looks at the layers, not an edit
  doc.value = setGroupFolded(doc.value, id, collapsed)
}

function addAdjustment(type) {
  if (!doc.value) return
  const made = addAdjustmentLayer(doc.value, type, selectedId.value)
  const sel = selectedLayer.value
  const next = sel?.parent ? setLayer(made.doc, made.id, { parent: sel.parent }) : made.doc
  const layer = next.layers.find((l) => l.id === made.id)
  commit(next, `New ${ADJUSTMENT_NAMES[type]} layer`, { op: adjustmentOp(layer.adjustment) })
  selectedId.value = made.id
  editMask.value = false
  dock.value?.show('properties')
}

// An adjustment layer as her op ({ op: 'adjustment', type, params }); the eight sliders have none.
function adjustmentOp(a) {
  if (!a || a.type === 'adjust') return null
  if (a.type === 'curves') return { op: 'adjustment', type: 'curves', params: { channel: 'rgb', points: a.params.rgb } }
  if (a.type === 'hsl') {
    const [range, v] = Object.entries(a.params.ranges || {}).find(([, r]) => r.hue || r.sat || r.light) || ['all', { hue: 0, sat: 0, light: 0 }]
    return { op: 'adjustment', type: 'hsl', params: { range, ...v } }
  }
  return { op: 'adjustment', type: a.type, params: a.params }
}

function onAdjustment(params, final) {
  const l = selectedLayer.value
  if (!l || l.kind !== 'adjustment') return
  const next = setAdjustment(doc.value, l.id, params)
  if (!final) return (doc.value = next)
  commit(next, ADJUSTMENT_NAMES[l.adjustment.type] || 'Adjustment')
  const op = adjustmentOp({ ...l.adjustment, params })
  if (op) rerecordLast((s) => s.op === 'adjustment' && s.type === op.type, op)
}

function onAdjust(key, value, final) {
  if (!doc.value) return
  const next = { ...doc.value, adjust: { ...doc.value.adjust, [key]: value } }
  if (final) commit(next, 'Adjusted', { op: { op: 'adjust', values: { [key]: value } } })
  else doc.value = next
}

function resetAdjust() {
  if (doc.value) commit({ ...doc.value, adjust: emptyAdjust() }, 'Adjust reset')
}

function onText(fields, final) {
  const l = selectedLayer.value
  if (!l || l.kind !== 'text') return
  const next = setLayer(doc.value, l.id, fields)
  if (final) commit(next, 'Type changed')
  else doc.value = next
}

async function addMask(id = selectedId.value) {
  const d = doc.value
  if (!d || !id) return
  try {
    const sel = await canvas.value.selectionMask(d)
    const mask = sel || whiteMask(d.width, d.height)
    commit(setMask(d, id, canvas.value.maskPngFrom(mask, d.width, d.height)), sel ? 'Mask from the selection' : 'Mask added', sel ? { op: { op: 'mask', from: 'selection' } } : {})
    selectedId.value = id
    editMask.value = true
  } catch (reason) {
    fail(reason)
  }
}

async function invertLayerMask(id = selectedId.value) {
  const d = doc.value
  try {
    const m = await canvas.value.maskOf(d, id)
    if (m) commit(setMask(d, id, canvas.value.maskPngFrom(invertMask(m), d.width, d.height)), 'Mask inverted', { op: { op: 'mask', from: 'invert' } })
  } catch (reason) {
    fail(reason)
  }
}

function deleteLayerMask(id = selectedId.value) {
  if (viewMask.value === id) viewMask.value = ''
  commit(setMask(doc.value, id, null), 'Mask deleted', { op: { op: 'mask', from: 'clear' } })
}

// The chosen layer onto the one under it: one picture layer with the lower one's name and place.
async function mergeDown() {
  if (!canMergeDown.value) return
  const d = doc.value
  const i = selectedIndex.value
  const upper = d.layers[i]
  const lower = d.layers[i - 1]
  busy.value = 'work'
  try {
    const lp = await canvas.value.pixelsOf(d, lower.id)
    const lm = await canvas.value.maskOf(d, lower.id)
    const um = await canvas.value.maskOf(d, upper.id)
    const entries = [{ id: 'lower', kind: 'image', pixels: lp, x: lower.x, y: lower.y, opacity: 1, blend: 'normal', visible: true, mask: lm, styles: lower.styles }]
    if (upper.visible) {
      if (upper.kind === 'adjustment') entries.push({ id: 'upper', kind: 'adjustment', adjustment: upper.adjustment, opacity: upper.opacity, visible: true, mask: um, clipped: upper.clipped })
      else entries.push({ id: 'upper', kind: 'image', pixels: await canvas.value.pixelsOf(d, upper.id), x: upper.x, y: upper.y, opacity: upper.opacity, blend: upper.blend, visible: true, mask: um, styles: upper.styles, clipped: upper.clipped })
    }
    const pixels = engine3.compose3(entries, d.width, d.height)
    const { text, textBox, styles, clipped, ...rest } = lower
    const merged = { ...rest, kind: 'image', png: canvas.value.pngFrom(pixels), x: 0, y: 0, mask: null }
    const layers = [...d.layers]
    layers.splice(i - 1, 2, merged)
    commit({ ...d, layers }, 'Merged down')
    selectedId.value = merged.id
  } catch (reason) {
    fail(reason)
  } finally {
    busy.value = ''
  }
}

async function flatten() {
  const d = doc.value
  if (!d) return
  busy.value = 'work'
  try {
    const pixels = await canvas.value.composite(d)
    const made = addFullLayer({ ...d, layers: [] }, canvas.value.pngFrom(pixels), 'Background')
    commit(made.doc, 'Flattened')
    selectedId.value = made.id
  } catch (reason) {
    fail(reason)
  } finally {
    busy.value = ''
  }
}

// ---------- layer styles ----------

function openStyle(id = selectedId.value, start = '') {
  const l = doc.value?.layers.find((x) => x.id === id)
  if (!l || l.kind === 'adjustment') return say('Choose a picture, text or group layer for a layer style.')
  styleDialog.value = { id, before: doc.value, start }
}

function previewStyle(styles) {
  const s = styleDialog.value
  if (s) doc.value = setStyles(s.before, s.id, styles)
}

function styleOps(layer, styles) {
  return STYLE_KEYS.filter((k) => styles?.[k] || layer.styles?.[k]).map((k) => ({ op: 'style', layer: layer.name, effect: k, set: styles?.[k] ? { ...styles[k] } : { on: false } }))
}

function okStyle(styles) {
  const s = styleDialog.value
  styleDialog.value = null
  if (!s) return
  doc.value = s.before
  const layer = s.before.layers.find((l) => l.id === s.id)
  commit(setStyles(s.before, s.id, styles), 'Layer style')
  for (const op of layer ? styleOps(layer, styles) : []) recordStep(op)
}

function cancelStyle() {
  const s = styleDialog.value
  styleDialog.value = null
  if (s) doc.value = s.before
}

function applyStylePreset(preset) {
  const l = selectedLayer.value
  if (!l || l.kind === 'adjustment') return say('Choose a picture, text or group layer first.')
  const styles = preset.styles ? JSON.parse(JSON.stringify(preset.styles)) : null
  commit(setStyles(doc.value, l.id, styles), preset.styles ? `Style: ${preset.name}` : 'Styles cleared')
  for (const op of styleOps(l, styles)) recordStep(op)
}

// ---------- selections ----------

function selectAll() {
  if (doc.value) commit({ ...doc.value, selection: { kind: 'rect', x: 0, y: 0, w: doc.value.width, h: doc.value.height } }, 'Select all', { op: { op: 'select', x: 0, y: 0, w: doc.value.width, h: doc.value.height } })
}

function deselect() {
  if (doc.value?.selection) commit({ ...doc.value, selection: null }, 'Deselected', { op: { op: 'select', none: true } })
}

function maskSelection(mask, label, op) {
  const d = doc.value
  if (!mask.some((v) => v > 0)) {
    say('Nothing was found to select.')
    return
  }
  commit({ ...d, selection: { kind: 'mask', png: canvas.value.maskPngFrom(mask, d.width, d.height) } }, label, op ? { op } : {})
}

async function smartSelect(kind, extra = {}) {
  const d = doc.value
  if (!d) return
  busy.value = 'work'
  try {
    await new Promise((r) => setTimeout(r, 30)) // let "Working" show before the heavy part
    if (kind === 'invert') {
      const sel = await canvas.value.selectionMask(d)
      if (!sel) return selectAll()
      return maskSelection(invertMask(sel), 'Selection inverted', { op: 'select', invert: true })
    }
    if (kind === 'refine') {
      const sel = await canvas.value.selectionMask(d)
      if (!sel) return
      return maskSelection(refineMask(sel, d.width, d.height, extra), 'Edge refined')
    }
    const pixels = await canvas.value.composite(d)
    if (kind === 'subject') {
      const sel = await canvas.value.selectionMask(d)
      const b = sel ? maskBounds(sel, d.width, d.height) : null
      const rect = b || { x: Math.round(d.width * 0.08), y: Math.round(d.height * 0.06), w: Math.round(d.width * 0.84), h: Math.round(d.height * 0.9) }
      maskSelection(grabCut(pixels, rect), 'Subject selected', { op: 'select', subject: true })
    } else if (kind === 'sky') maskSelection(skyMask(pixels), 'Sky selected', { op: 'select', sky: true })
    else if (kind === 'range') maskSelection(colorRange(pixels, primary.value, extra.fuzz), 'Colour range selected', { op: 'select', color: primary.value, fuzz: extra.fuzz })
  } catch (reason) {
    fail(reason)
  } finally {
    busy.value = ''
  }
}

// Q: the selection becomes a red-tinted mask he paints; Q again turns it back into a selection.
async function toggleQuickMask() {
  if (!doc.value) return
  if (quickMask.value) {
    quickMask.value = false
    canvas.value?.endQuickMask()
    return
  }
  quickMask.value = true
  if (!['brush', 'eraser'].includes(tool.value)) tool.value = 'brush'
  await canvas.value?.startQuickMask()
}

// A channel (or RGB's brightness) as the selection.
async function loadChannel(id) {
  const d = doc.value
  if (!d) return
  busy.value = 'work'
  try {
    const p = await canvas.value.composite(d)
    const mask = new Uint8Array(d.width * d.height)
    const k = { r: 0, g: 1, b: 2, alpha: 3 }[id]
    for (let j = 0, i = 0; j < mask.length; j++, i += 4) {
      const a = p.data[i + 3]
      mask[j] = k === 3 ? a : k === undefined ? ((0.299 * p.data[i] + 0.587 * p.data[i + 1] + 0.114 * p.data[i + 2]) * a) / 255 : (p.data[i + k] * a) / 255
    }
    maskSelection(mask, `${id === 'rgb' ? 'RGB' : id.toUpperCase()} channel loaded`)
  } catch (reason) {
    fail(reason)
  } finally {
    busy.value = ''
  }
}

async function openSelectMask() {
  const d = doc.value
  if (!d) return
  busy.value = 'work'
  try {
    workspace.value = { kind: 'mask', pixels: await canvas.value.composite(d), mask: await canvas.value.selectionMask(d) }
  } catch (reason) {
    fail(reason)
  } finally {
    busy.value = ''
  }
}

function doneSelectMask({ mask, output }) {
  workspace.value = null
  const d = doc.value
  const l = selectedLayer.value
  if (output === 'selection' || !l) return maskSelection(mask, 'Select and Mask')
  const png = canvas.value.maskPngFrom(mask, d.width, d.height)
  if (output === 'mask') return commit(setMask(d, l.id, png), 'Mask from Select and Mask')
  const made = duplicateLayer3(d, l.id)
  if (!made.id) return
  commit(setLayer(setMask(made.doc, made.id, png), l.id, { visible: false }), 'New layer with a mask')
  selectedId.value = made.id
}

// ---------- pixels of the chosen layer (fill, filters) ----------

async function imageTarget() {
  const d = doc.value
  const l = selectedLayer.value
  if (!d || !l || l.kind !== 'image') {
    say('Choose a picture layer first.')
    return null
  }
  const pixels = await canvas.value.pixelsOf(d, l.id)
  const sel = await canvas.value.selectionMask(d)
  const local = sel ? localMask(sel, d.width, d.height, l.x, l.y, pixels.width, pixels.height) : null
  return { d, l, pixels, local }
}

async function changePixels(label, work, op) {
  busy.value = 'work'
  try {
    await new Promise((r) => setTimeout(r, 30))
    const t = await imageTarget()
    if (!t) return
    const out = await work(t)
    if (out) commit(setLayer(doc.value, t.l.id, { png: canvas.value.pngFrom(out) }), label, op ? { op: { ...op, layer: t.l.name } } : {})
  } catch (reason) {
    fail(reason)
  } finally {
    busy.value = ''
  }
}

const fillColour = () => changePixels('Fill', ({ pixels, local }) => fillPixels(pixels, hexToRgba(primary.value), local))
const fillContent = () =>
  changePixels(
    'Content-aware fill',
    ({ pixels, local }) => {
      if (!local || !local.some((v) => v > 0)) {
        say('Select what to fill first.')
        return null
      }
      return contentAwareFill(pixels, local)
    },
    { op: 'fill', mode: 'content_aware' },
  )

// The v3 filters' settings that are distances shrink with the live look's scale.
const scaled = (v, s, keys = []) => Object.fromEntries(Object.entries(v).map(([k, x]) => [k, keys.includes(k) && typeof x === 'number' ? Math.max(k === 'cell' ? 1 : 0.3, x * s) : x]))
const v3 = (id, keys) => (p, s, m, v) => engine3[id](p, scaled(v, s, keys), m)
const amountOf = { blur: (v) => (v.radius / 15) * 100, sharpen: (v) => v.amount / 2, noise: (v) => v.amount / 0.3, vignette: (v) => Math.abs(v.amount), denoise: (v) => v.radius * 25 }

// fn(pixels, scale, mask, values) - scale is 1 for the real thing, smaller for the live look. v2 filters keep
// their own dialogs; the v3 ones go to engine3 by name. group: the Filter menu's list it sits in.
const FILTERS = {
  blur: { group: 'Blur', title: 'Gaussian Blur', label: 'Blur', fields: [{ key: 'radius', label: 'Radius', min: 0.5, max: 60, step: 0.5, value: 4 }], fn: (p, s, m, v) => gaussianBlur(p, Math.max(0.3, v.radius * s), m) },
  motionBlur: {
    group: 'Blur',
    title: 'Motion Blur',
    fields: [
      { key: 'angle', label: 'Angle', min: -180, max: 180, step: 1, value: 0 },
      { key: 'distance', label: 'Distance', min: 1, max: 200, step: 1, value: 12 },
    ],
    fn: v3('motionBlur', ['distance']),
  },
  radialBlur: {
    group: 'Blur',
    title: 'Radial Blur',
    fields: [
      { key: 'mode', label: 'Method', type: 'select', options: ['spin', 'zoom'], value: 'spin' },
      { key: 'amount', label: 'Amount', min: 1, max: 100, step: 1, value: 10 },
    ],
    fn: v3('radialBlur'),
  },
  surfaceBlur: {
    group: 'Blur',
    title: 'Surface Blur',
    fields: [
      { key: 'radius', label: 'Radius', min: 1, max: 50, step: 1, value: 5 },
      { key: 'threshold', label: 'Threshold', min: 2, max: 100, step: 1, value: 15 },
    ],
    fn: v3('surfaceBlur', ['radius']),
  },
  lensBlur: {
    group: 'Blur',
    title: 'Lens Blur',
    note: 'With a selection, the selected part stays sharp.',
    fields: [{ key: 'radius', label: 'Radius', min: 1, max: 60, step: 1, value: 10 }],
    fn: (p, s, m, v) => engine3.lensBlur(p, { radius: Math.max(0.3, v.radius * s), depthMask: m ? invertMask(m) : null }, null),
  },
  sharpen: {
    group: 'Sharpen',
    title: 'Unsharp Mask',
    label: 'Sharpened',
    fields: [
      { key: 'amount', label: 'Amount %', min: 0, max: 300, step: 1, value: 80 },
      { key: 'radius', label: 'Radius', min: 0.3, max: 10, step: 0.1, value: 1.2 },
      { key: 'threshold', label: 'Threshold', min: 0, max: 60, step: 1, value: 2 },
    ],
    fn: (p, s, m, v) => unsharp(p, { amount: v.amount, radius: Math.max(0.3, v.radius * s), threshold: v.threshold }, m),
  },
  highPass: { group: 'Sharpen', title: 'High Pass', fields: [{ key: 'radius', label: 'Radius', min: 0.5, max: 100, step: 0.5, value: 10 }], fn: v3('highPass', ['radius']) },
  noise: {
    group: 'Noise',
    title: 'Add Noise',
    label: 'Noise',
    fields: [
      { key: 'amount', label: 'Amount', min: 0, max: 100, step: 1, value: 8 },
      { key: 'mono', label: 'Monochrome', type: 'check', value: false },
    ],
    fn: (p, s, m, v) => addNoise(p, v.amount, 7, m, !!v.mono),
  },
  addGrain: {
    group: 'Noise',
    title: 'Film Grain',
    fields: [
      { key: 'amount', label: 'Amount', min: 0, max: 100, step: 1, value: 20 },
      { key: 'size', label: 'Size', min: 1, max: 5, step: 0.5, value: 1 },
    ],
    fn: v3('addGrain', ['size']),
  },
  denoise: { group: 'Noise', title: 'Reduce Noise', label: 'Denoised', fields: [{ key: 'radius', label: 'Radius', min: 1, max: 6, step: 1, value: 1 }], fn: (p, s, m, v) => median(p, Math.max(1, Math.round(v.radius * s)), m) },
  emboss: {
    group: 'Stylize',
    title: 'Emboss',
    fields: [
      { key: 'angle', label: 'Angle', min: -180, max: 180, step: 1, value: 135 },
      { key: 'height', label: 'Height', min: 1, max: 10, step: 1, value: 3 },
      { key: 'amount', label: 'Amount %', min: 1, max: 500, step: 1, value: 100 },
    ],
    fn: v3('emboss', ['height']),
  },
  findEdges: { group: 'Stylize', title: 'Find Edges', fields: [], fn: v3('findEdges') },
  oilPaint: {
    group: 'Stylize',
    title: 'Oil Paint',
    fields: [
      { key: 'radius', label: 'Brush', min: 1, max: 10, step: 1, value: 3 },
      { key: 'levels', label: 'Levels', min: 4, max: 60, step: 1, value: 20 },
    ],
    fn: v3('oilPaint', ['radius']),
  },
  pixelate: { group: 'Pixelate', title: 'Mosaic', fields: [{ key: 'cell', label: 'Cell size', min: 2, max: 200, step: 1, value: 8 }], fn: v3('pixelate', ['cell']) },
  develop: {
    group: 'Camera',
    title: 'Develop',
    fields: [
      { key: 'temp', label: 'Temperature', min: -100, max: 100, step: 1, value: 0 },
      { key: 'tint', label: 'Tint', min: -100, max: 100, step: 1, value: 0 },
      { key: 'clarity', label: 'Clarity', min: -100, max: 100, step: 1, value: 0 },
      { key: 'dehaze', label: 'Dehaze', min: -100, max: 100, step: 1, value: 0 },
      { key: 'texture', label: 'Texture', min: -100, max: 100, step: 1, value: 0 },
    ],
    fn: v3('develop'),
  },
  vignette: {
    group: 'Camera',
    title: 'Vignette',
    label: 'Vignette',
    fields: [
      { key: 'amount', label: 'Amount', min: -100, max: 100, step: 1, value: 40 },
      { key: 'size', label: 'Size', min: 0, max: 100, step: 1, value: 50 },
    ],
    fn: (p, s, m, v) => vignette(p, { amount: v.amount, size: v.size }, m),
  },
}

// The filter as her op (v2 filters by an amount 0-100, the v3 ones with their settings).
function filterOp(kind, v) {
  if (amountOf[kind]) return { op: 'filter', name: kind, amount: Math.max(1, Math.min(100, Math.round(amountOf[kind](v)))) }
  const { mode, ...rest } = v
  return { op: 'filter', name: kind, params: mode ? { ...rest, mode } : rest }
}

function openFilter(kind) {
  const f = FILTERS[kind]
  const l = selectedLayer.value
  if (!f) return
  if (!l || l.kind !== 'image') return say('Choose a picture layer first.')
  const apply = (v) => changePixels(f.label || f.title, ({ pixels, local }) => f.fn(pixels, 1, local, v), filterOp(kind, v))
  if (!f.fields.length) return apply({})
  openDialog({
    title: f.title,
    note: f.note,
    fields: f.fields,
    ok: 'Apply',
    preview: (v) => ({ id: l.id, fn: (p, s, m) => f.fn(p, s, m, v) }),
    apply,
  })
}

// ---------- Liquify ----------

async function openLiquify() {
  const l = selectedLayer.value
  if (!l || l.kind !== 'image') return say('Choose a picture layer to liquify.')
  busy.value = 'work'
  try {
    workspace.value = { kind: 'liquify', id: l.id, name: l.name, origin: { x: l.x, y: l.y }, pixels: await canvas.value.pixelsOf(doc.value, l.id) }
  } catch (reason) {
    fail(reason)
  } finally {
    busy.value = ''
  }
}

async function doneLiquify({ mesh, strokes }) {
  const w = workspace.value
  workspace.value = null
  if (!w) return
  busy.value = 'work'
  try {
    await new Promise((r) => setTimeout(r, 30))
    const out = engine3.applyMesh(w.pixels, mesh)
    // recorded in engine3's own words (her hand reads push; an old 'forward' still means push)
    const op = { op: 'liquify', layer: w.name, strokes }
    commit(setLayer(doc.value, w.id, { png: canvas.value.pngFrom(out) }), 'Liquify', { op })
  } catch (reason) {
    fail(reason)
  } finally {
    busy.value = ''
  }
}

// ---------- artboards, guides, grid ----------

// A new artboard; the canvas grows (from its top-left) when the artboard reaches past it.
async function newArtboard({ name: title, w, h }) {
  const d = doc.value
  if (!d) return
  const { doc: next, board } = addArtboard(d, { name: title, w, h })
  let out = next
  const needW = Math.max(d.width, board.x + board.w)
  const needH = Math.max(d.height, board.y + board.h)
  if (needW > d.width || needH > d.height) {
    if (needW > 12000 || needH > 12000) return say('That artboard would make the canvas too big.')
    out = { ...(await canvas.value.transformDoc(next, { op: 'canvas', width: needW, height: needH, x: 0, y: 0 })), artboards: next.artboards, guides: next.guides }
  }
  commit(out, `Artboard ${board.w}×${board.h}`, { op: { op: 'artboard', name: board.name, w: board.w, h: board.h } })
}

function onGuideAdd(axis, at) {
  commit(addGuide(doc.value, axis, at), 'Guide added', { op: { op: 'guide', axis: axis === 'x' ? 'v' : 'h', at } })
}

function setGrid(fields, label = '') {
  const next = { ...doc.value, grid: { ...doc.value.grid, ...fields } }
  if (label) commit(next, label)
  else doc.value = next
}

// ---------- paths ----------

function pathPoints(p) {
  return p.closed ? [...p.points, p.points[0]] : p.points
}

async function strokePath(id) {
  const p = doc.value?.paths?.find((x) => x.id === id)
  if (!p) return
  const l = selectedLayer.value
  const op = { op: 'brush', points: pathPoints(p).map(([x, y]) => [Math.round(x), Math.round(y)]), size: brush.size, hardness: brush.hardness, opacity: brush.opacity, color: primary.value, ...(l?.kind === 'image' ? { layer: l.name } : {}) }
  try {
    await canvas.value.selectionMask(doc.value)
    const out = await ops.applyOp(doc.value, op, { ...helpers, play: undefined })
    commit(out.doc, 'Path stroked', { op })
  } catch (reason) {
    fail(reason)
  }
}

function fillPath(id) {
  const p = doc.value?.paths?.find((x) => x.id === id)
  if (!p) return
  const d = doc.value
  const pixels = drawShape({ data: new Uint8ClampedArray(d.width * d.height * 4), width: d.width, height: d.height }, { kind: 'polygon', points: p.points, fill: primary.value, stroke: null, width: 0 })
  const made = addFullLayer(d, canvas.value.pngFrom(pixels), `${p.name} fill`, selectedId.value)
  commit(made.doc, 'Path filled')
  selectedId.value = made.id
}

function pathSelection(id) {
  const p = doc.value?.paths?.find((x) => x.id === id)
  if (p) commit({ ...doc.value, selection: { kind: 'lasso', points: p.points.map(([x, y]) => [Math.round(x), Math.round(y)]) } }, 'Path to selection')
}

// ---------- dialogs ----------

function openDialog(spec) {
  const values = {}
  for (const f of spec.fields) values[f.key] = f.value
  dialog.value = { ...spec, values }
  if (spec.preview) schedulePreview()
}

function schedulePreview() {
  clearTimeout(previewTimer)
  previewTimer = setTimeout(() => {
    const d = dialog.value
    if (d?.preview) canvas.value?.setPreview(d.preview({ ...d.values }))
  }, 120)
}

function dialogInput(f, raw) {
  const d = dialog.value
  if (!d) return
  d.values[f.key] = f.type === 'check' ? !!raw : f.type === 'select' || f.type === 'text' ? raw : Number(raw)
  d.onChange?.(f.key, d.values)
  if (d.preview) schedulePreview()
}

function closeDialog() {
  clearTimeout(previewTimer)
  if (dialog.value?.preview) canvas.value?.setPreview(null)
  dialog.value = null
}

async function applyDialog() {
  const d = dialog.value
  if (!d) return
  const values = { ...d.values }
  closeDialog()
  await d.apply(values)
}

function openSizeDialog(kind) {
  const d = doc.value
  if (!d) return
  const fields = [
    { key: 'width', label: 'Width (px)', type: 'number', min: 1, max: 12000, step: 1, value: d.width },
    { key: 'height', label: 'Height (px)', type: 'number', min: 1, max: 12000, step: 1, value: d.height },
  ]
  if (kind === 'image') fields.push({ key: 'ratio', label: 'Keep proportions', type: 'check', value: true })
  else
    fields.push({
      key: 'anchor',
      label: 'Anchor',
      type: 'select',
      value: 'center',
      options: ['top-left', 'top', 'top-right', 'left', 'center', 'right', 'bottom-left', 'bottom', 'bottom-right'],
    })
  openDialog({
    title: kind === 'image' ? 'Image size' : 'Canvas size',
    fields,
    ok: 'Resize',
    onChange: (key, v) => {
      if (kind !== 'image' || !v.ratio || (key !== 'width' && key !== 'height')) return
      const r = keepRatio(d.width, d.height, v.width, v.height, key === 'width' ? 'w' : 'h')
      v.width = r.w
      v.height = r.h
    },
    apply: (v) => {
      if (kind === 'image') recordStep({ op: 'resize', width: Math.round(v.width), height: Math.round(v.height) })
      return canvas.value.runTransform(
        kind === 'image' ? { op: 'resize', width: Math.round(v.width), height: Math.round(v.height) } : { op: 'canvas', width: Math.round(v.width), height: Math.round(v.height), anchor: v.anchor },
      )
    },
  })
}

// ---------- the Window menu ----------

function togglePanel(id) {
  const hidden = viewPrefs.hidden
  if (hidden.includes(id)) {
    viewPrefs.hidden = hidden.filter((x) => x !== id)
    nextTick(() => dock.value?.show(id))
  } else if (dock.value?.activeOf(id)) viewPrefs.hidden = [...hidden, id]
  else dock.value?.show(id)
}

function resetWorkspace() {
  viewPrefs.hidden = []
  viewPrefs.rulers = true
  viewPrefs.guides = true
  dock.value?.reset()
}

// ---------- the menus ----------

function startFreeTransform() {
  if (!doc.value) return
  tool.value = tool.value === 'crop' ? 'move' : tool.value
  canvas.value?.startFreeTransform()
}

function runMenu(id) {
  showRecent.value = false
  const d = doc.value
  if (id.startsWith('adj-')) return addAdjustment(id.slice(4))
  if (id.startsWith('filter-')) return openFilter(id.slice(7))
  if (id.startsWith('panel-')) return togglePanel(id.slice(6))
  if (id.startsWith('artboard-') && id !== 'artboard-custom') {
    const p = PRESETS.find((x) => `artboard-${x.id}` === id)
    return p && newArtboard({ name: p.label, w: p.w, h: p.h })
  }
  switch (id) {
    case 'new':
      return openDialog({
        title: 'New',
        ok: 'Create',
        fields: [
          { key: 'width', label: 'Width (px)', type: 'number', min: 1, max: 12000, step: 1, value: d?.width || 1920 },
          { key: 'height', label: 'Height (px)', type: 'number', min: 1, max: 12000, step: 1, value: d?.height || 1080 },
          { key: 'background', label: 'Background', type: 'select', value: 'white', options: ['white', 'black', 'transparent'] },
        ],
        apply: newDocument,
      })
    case 'open':
      return openPicked()
    case 'recent':
      return toggleRecent()
    case 'save':
      return save()
    case 'export-as':
      return d && (exportOpen.value = true)
    case 'export-png':
      return exportAs('png')
    case 'export-jpeg':
      return openDialog({ title: 'Export as JPG', ok: 'Export', fields: [{ key: 'quality', label: 'Quality', min: 60, max: 100, step: 1, value: 90 }], apply: (v) => exportAs('jpeg', v.quality) })
    case 'undo':
      return undo()
    case 'redo':
      return redo()
    case 'fill-colour':
      return fillColour()
    case 'fill-content':
      return fillContent()
    case 'free-transform':
      return startFreeTransform()
    case 'artboard-custom':
      return openDialog({
        title: 'New artboard',
        ok: 'Add',
        fields: [
          { key: 'name', label: 'Name', type: 'text', value: `Artboard ${(d?.artboards?.length || 0) + 1}` },
          { key: 'width', label: 'Width (px)', type: 'number', min: 1, max: 12000, step: 1, value: 1080 },
          { key: 'height', label: 'Height (px)', type: 'number', min: 1, max: 12000, step: 1, value: 1080 },
        ],
        apply: (v) => newArtboard({ name: v.name, w: v.width, h: v.height }),
      })
    case 'crop':
      return canvas.value?.applyCrop()
    case 'rotate-left':
      recordStep({ op: 'rotate', turns: 3 })
      return canvas.value?.rotate(-1)
    case 'rotate-right':
      recordStep({ op: 'rotate', turns: 1 })
      return canvas.value?.rotate(1)
    case 'flip-h':
      recordStep({ op: 'flip', axis: 'h' })
      return canvas.value?.flip('h')
    case 'flip-v':
      recordStep({ op: 'flip', axis: 'v' })
      return canvas.value?.flip('v')
    case 'image-size':
      return openSizeDialog('image')
    case 'canvas-size':
      return openSizeDialog('canvas')
    case 'layer-new':
      return addLayer()
    case 'layer-duplicate':
      return selectedId.value && onDuplicate(selectedId.value)
    case 'layer-delete':
      return selectedId.value && onRemove(selectedId.value)
    case 'layer-group':
      return onGroup()
    case 'layer-ungroup':
      return onUngroup()
    case 'layer-clip':
      return onClip()
    case 'layer-style':
      return openStyle()
    case 'mask-add':
      return addMask()
    case 'mask-invert':
      return invertLayerMask()
    case 'mask-delete':
      return deleteLayerMask()
    case 'merge-down':
      return mergeDown()
    case 'flatten':
      return flatten()
    case 'select-all':
      return selectAll()
    case 'select-none':
      return deselect()
    case 'select-invert':
      return smartSelect('invert')
    case 'select-subject':
      return smartSelect('subject')
    case 'select-sky':
      return smartSelect('sky')
    case 'select-range':
      return openDialog({
        title: 'Colour range',
        ok: 'Select',
        note: `Picks pixels near the primary colour ${primary.value}.`,
        fields: [{ key: 'fuzz', label: 'Fuzziness', min: 0, max: 200, step: 1, value: 40 }],
        apply: (v) => smartSelect('range', v),
      })
    case 'select-refine':
      return openDialog({
        title: 'Refine edge',
        ok: 'Refine',
        fields: [
          { key: 'smooth', label: 'Smooth', min: 0, max: 20, step: 1, value: 2 },
          { key: 'feather', label: 'Feather', min: 0, max: 60, step: 0.5, value: 2 },
          { key: 'shift', label: 'Shift edge', min: -20, max: 20, step: 1, value: 0 },
        ],
        apply: (v) => smartSelect('refine', v),
      })
    case 'quick-mask':
      return toggleQuickMask()
    case 'select-mask':
      return openSelectMask()
    case 'liquify':
      return openLiquify()
    case 'zoom-in':
      return canvas.value?.zoomBy(1.25)
    case 'zoom-out':
      return canvas.value?.zoomBy(0.8)
    case 'zoom-fit':
      return canvas.value?.resetView()
    case 'zoom-100':
      return canvas.value?.actualSize()
    case 'rulers':
      return (viewPrefs.rulers = !viewPrefs.rulers)
    case 'guides-show':
      return (viewPrefs.guides = !viewPrefs.guides)
    case 'guides-lock':
      return (viewPrefs.lock = !viewPrefs.lock)
    case 'guide-new':
      return openDialog({
        title: 'New guide',
        ok: 'Add',
        fields: [
          { key: 'axis', label: 'Orientation', type: 'select', options: ['vertical', 'horizontal'], value: 'vertical' },
          { key: 'at', label: 'Position (px)', type: 'number', min: -12000, max: 24000, step: 1, value: Math.round((d?.width || 0) / 2) },
        ],
        apply: (v) => onGuideAdd(v.axis === 'vertical' ? 'x' : 'y', v.at),
      })
    case 'guides-clear':
      return d?.guides?.length && commit({ ...d, guides: [] }, 'Guides cleared', { op: { op: 'guides_clear' } })
    case 'grid':
      return d && setGrid({ show: !d.grid?.show })
    case 'grid-size':
      return openDialog({
        title: 'Grid',
        ok: 'Set',
        fields: [
          { key: 'size', label: 'Every (px)', type: 'number', min: 2, max: 2000, step: 1, value: d?.grid?.size || 50 },
          { key: 'snap', label: 'Snap to it', type: 'check', value: d?.grid?.snap !== false },
        ],
        apply: (v) => setGrid({ size: Math.max(2, Math.round(v.size)), snap: !!v.snap, show: true }, 'Grid size'),
      })
    case 'snap':
      return (viewPrefs.snap = !viewPrefs.snap)
    case 'workspace-reset':
      return resetWorkspace()
  }
}

// ---------- colours, the adjustment layer's histogram, the thumbnail ----------

function onColour({ primary: p, secondary: s }) {
  primary.value = p
  secondary.value = s
}

watch(
  () => [selectedLayer.value?.id, selectedLayer.value?.adjustment?.type, doc.value?.layers],
  () => {
    clearTimeout(histTimer)
    const l = selectedLayer.value
    if (!l || l.kind !== 'adjustment' || !['curves', 'levels'].includes(l.adjustment?.type)) {
      histogram.value = null
      return
    }
    histTimer = setTimeout(async () => {
      try {
        histogram.value = await canvas.value?.histogramBelow(l.id)
      } catch {
        histogram.value = null
      }
    }, 300)
  },
)

// NAVIGATOR and CHANNELS read a small picture of the canvas, a moment after it changes.
watch(
  () => [doc.value, channel.value],
  () => {
    clearTimeout(thumbTimer)
    if (!doc.value) {
      thumb.value = null
      return
    }
    if (channel.value) return
    thumbTimer = setTimeout(() => {
      try {
        thumb.value = canvas.value?.thumbnail(220) || null
      } catch {
        thumb.value = null
      }
    }, 600)
  },
)

// ---------- actions: play, batch, keep ----------

async function loadActions() {
  try {
    actions.value = cleanActions(await photoActionsLoad())
  } catch {
    actions.value = []
  }
}

async function setActions(list) {
  actions.value = cleanActions(list)
  try {
    await photoActionsSave(actions.value)
  } catch (reason) {
    say(`The actions could not be kept: ${reason?.message || reason}`)
  }
}

function startRecording(actionName) {
  const nameOk = actions.value.some((a) => a.name === actionName) ? uniqueActionName(actions.value, actionName) : actionName
  recording.value = nameOk
  recSteps.value = []
  actions.value = [...actions.value, { name: nameOk, steps: [] }]
  say(`Recording "${nameOk}" - what you and ${props.companion} do is kept until you press stop.`)
}

async function stopRecording() {
  const n = recording.value
  if (!n) return
  recording.value = ''
  const steps = recSteps.value
  recSteps.value = []
  await setActions(actions.value.map((a) => (a.name === n ? { ...a, steps } : a)))
  say(`"${n}" kept with ${steps.length} step${steps.length === 1 ? '' : 's'}.`)
}

// A list of ops on the doc, one after the other -> the doc after all of them (nothing committed here).
async function runActionSteps(steps, start = doc.value, h = helpers) {
  let current = start
  for (let i = 0; i < steps.length; i++) {
    try {
      current = (await ops.applyOp(current, steps[i], h)).doc
    } catch (reason) {
      throw new Error(`Step ${i + 1} (${steps[i]?.op}) failed: ${reason?.message || reason}`)
    }
  }
  return current
}

async function playAction(actionName) {
  const a = actions.value.find((x) => x.name === actionName)
  if (!a || !doc.value) return
  actionBusy.value = 'play'
  busy.value = 'work'
  try {
    await canvas.value.selectionMask(doc.value)
    const next = await runActionSteps(a.steps, doc.value, { ...helpers, play: undefined })
    commit(next, `Action: ${a.name}`, { op: { op: 'run_action', name: a.name } })
  } catch (reason) {
    fail(reason)
  } finally {
    actionBusy.value = ''
    busy.value = ''
  }
}

// Every picture in the folder: opened apart, the action's steps run on it, flattened and written into the
// folder's PHOTO out (the originals never touched). -> how many were done.
async function batchFolder({ action, folder, steps }) {
  const list = steps || actions.value.find((a) => a.name === action)?.steps
  if (!list?.length) throw new Error(`The action ${action} has no steps.`)
  const paths = (await photoBatchList(folder)) || []
  const scratchHelpers = { ...helpers, play: undefined, selectedId: () => '', selectionMask: (d) => canvas.value.selectionMaskNow(d) }
  let count = 0
  for (const path of paths) {
    if (/\.psd$/i.test(path)) continue
    const got = await photoRead(path)
    const pic = await toPng(got.png)
    const scratch = docFromPicture({ ...pic, width: pic.width || got.width, height: pic.height || got.height }, stem(path))
    const done = await runActionSteps(list, scratch, scratchHelpers)
    const format = formatForPath(path) || 'png'
    const image = await canvas.value.flattenDoc(done, { format, quality: 92 })
    await photoBatchWrite({ folder, name: stem(path), image, format })
    count += 1
  }
  return count
}

async function batchAction(actionName) {
  try {
    const folder = await photoPickFolder()
    if (!folder) return
    actionBusy.value = 'batch'
    const count = await batchFolder({ action: actionName, folder })
    say(`${count} picture${count === 1 ? '' : 's'} done into "${folder}\\PHOTO out".`)
  } catch (reason) {
    fail(reason)
  } finally {
    actionBusy.value = ''
  }
}

function removeAction(actionName) {
  setActions(actions.value.filter((a) => a.name !== actionName))
}

// ---------- the companion ----------

// Drawing something new (the companion's generate_layer, the one op no native tool can do): the picture goes
// on Castle's image queue, on the member's own key, and the drawing comes back with what it cost.
const DRAW_USD = pictureCost(null, 'medium')
const DRAW_SIDE = 2048

// base64 PNG -> base64 PNG no larger than side x side (the image provider never needs more)
function shrinkBase64(b64, side) {
  return new Promise((resolve, reject) => {
    const img = new Image()
    img.onload = () => {
      const scale = Math.min(1, side / Math.max(img.naturalWidth, img.naturalHeight))
      if (scale >= 1) return resolve(b64)
      const c = document.createElement('canvas')
      c.width = Math.max(1, Math.round(img.naturalWidth * scale))
      c.height = Math.max(1, Math.round(img.naturalHeight * scale))
      const ctx = c.getContext('2d')
      ctx.imageSmoothingQuality = 'high'
      ctx.drawImage(img, 0, 0, c.width, c.height)
      resolve(c.toDataURL('image/png').split(',')[1])
    }
    img.onerror = () => reject(new Error('the picture could not be read'))
    img.src = `data:image/png;base64,${b64}`
  })
}

async function generateNew({ kind, image, prompt }) {
  const d = doc.value
  const picture = `data:image/png;base64,${await shrinkBase64(image, DRAW_SIDE)}`
  const job = await photoRedraw(kind, { prompt, picture, width: d?.width || 1024, height: d?.height || 1024 })
  say(`Drawing "${prompt}" with your image provider${DRAW_USD ? ` (about $${DRAW_USD.toFixed(2)} on your own key)` : ''}…`)
  const started = Date.now()
  while (Date.now() - started < 10 * 60 * 1000) {
    await new Promise((resolve) => setTimeout(resolve, 2500))
    let jobs = []
    try {
      jobs = await imageJobs()
    } catch {
      continue
    }
    const found = jobs.find((j) => j.id === job.id)
    if (!found) throw new Error('the drawing is no longer on the picture queue')
    if (found.phase === 'completed') {
      try {
        return { png: await photoResult(job.id), costUsd: DRAW_USD || 0 }
      } finally {
        imageDismiss(job.id).catch(() => {})
      }
    }
    // a refusal is one plain line
    if (found.phase === 'prompt-saved') throw new Error(String(found.message || 'the picture was not drawn').split('\n')[0])
  }
  throw new Error('the drawing took too long')
}

const helpers = {
  flatten: (d) => canvas.value.flattenDoc(d, { withAdjust: true }),
  transform: (d, op) => canvas.value.transformDoc(d, op),
  pixelsOf: (d, id) => canvas.value.pixelsOf(d, id),
  maskOf: (d, id) => canvas.value.maskOf(d, id),
  pngFrom: async (pixels) => canvas.value.pngFrom(pixels),
  maskPngFrom: async (mask, w, h) => canvas.value.maskPngFrom(mask, w, h),
  composite: (d) => canvas.value.composite(d),
  selectionMask: (d) => canvas.value.selectionMaskNow(d),
  play: (points, kind) => canvas.value.play(points, kind),
  selectedId: () => selectedId.value,
  // v3
  composite3: (d) => canvas.value.composite(d),
  exportArtboards: (d, sizes, opts) => exportArtboards(d, sizes, opts),
  exportMany: (files) => photoExportMany({ files }),
  runActionSteps: (steps) => runActionSteps(steps),
  batchFolder: (args) => batchFolder(args),
  actions: { get: () => actions.value, set: (list) => setActions(list) },
  // the name on the layers the companion's hand makes
  get who() {
    return props.companion
  },
}

// The companion's ask, shared by the chat and a project terminal's door edit (which passes the native-only api)
async function runAsk(message, api = { photoAsk, photoGenerate: generateNew }) {
  canvas.value.cancelTransform()
  // her ops read the selection at once: have it decoded first
  await canvas.value.selectionMask(doc.value)
  const flat = await canvas.value.flattenBase64()
  // while an action records, her ops are kept too
  let asked = []
  const watched = {
    ...api,
    photoAsk: async (a) => {
      const reply = await api.photoAsk(a)
      asked = Array.isArray(reply?.ops) ? reply.ops : []
      return reply
    },
  }
  const answer = await ops.ask({ doc: doc.value, flat, message }, watched, helpers)
  for (const result of answer?.results ?? []) {
    if (!result?.doc) continue
    commit(result.doc, result.label, { hers: true })
    pills.value.push({ id: `pill-${++pillCount}`, label: result.label, undo: result.undo })
  }
  if (recording.value && answer?.results?.length) for (const op of asked) if (!['generate_layer', 'record_action', 'batch'].includes(op?.op)) recordStep(op)
  if (answer?.say) sayLines.value.push(answer.say)
  return answer
}

async function onAsk(message) {
  if (asking.value) return
  if (!doc.value) {
    problem.value = `Open a photo first, then tell ${props.companion} what to change.`
    return
  }
  asking.value = true
  problem.value = ''
  try {
    await runAsk(message)
  } catch (reason) {
    fail(reason)
  } finally {
    asking.value = false
  }
}

// A picture handed in: from DEV's FILES (opened with Save back), or a project terminal's --photo-edit (opened,
// edited by the companion with native tools only, saved back, the terminal answered)
async function onRequest(raw) {
  const request = readRequest(raw)
  if (!request) return
  if (request.kind === 'open') {
    await openPath(request.path)
    if (request.saveBack) saveBackPath.value = request.path
    return
  }
  const where = { id: request.id, file: request.path }
  if (dirty.value || asking.value) {
    await photoDoorDone(doorDone(where, { ok: false, reason: 'PHOTO has unsaved work open - save it first, then ask again.' })).catch(() => {})
    return
  }
  try {
    await openPath(request.path)
    if (!doc.value) throw new Error('The picture could not be opened.')
    saveBackPath.value = request.path
    asking.value = true
    const answer = await runAsk(request.say, nativeOnly({ photoAsk, photoGenerate: generateNew }))
    await writeBack()
    await photoDoorDone(doorDone(where, { ok: true, say: answer?.say }))
  } catch (reason) {
    fail(reason)
    await photoDoorDone(doorDone(where, { ok: false, reason: String(reason?.message ?? reason) })).catch(() => {})
  } finally {
    asking.value = false
  }
}
// A request can arrive before PHOTO was ever opened (the core keeps it until it is taken) or while it is open
// (heard at once); each one is handled once, even if PHOTO is closed and opened again (takeOnce remembers
// outside the room)
function takeRequest(request) {
  if (request && takeOnce(request.at)) onRequest(request)
}

async function onPillUndo(pill) {
  if (!doc.value) return
  try {
    const out = await ops.undoPill(doc.value, pill, helpers)
    const at = pills.value.findIndex((p) => p.id === pill.id)
    if (out?.doc && out.doc !== doc.value) commit(out.doc, `Undid ${pill.label}`)
    if (out?.note) say(out.note)
    if (at >= 0) pills.value.splice(at, out?.dropAfter ? pills.value.length - at : 1)
  } catch (reason) {
    fail(reason)
  }
}

// ---------- keys ----------

const typing = (el) => el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT' || el.isContentEditable)

const CTRL_KEYS = {
  z: () => undo(),
  y: () => redo(),
  a: () => selectAll(),
  d: () => deselect(),
  j: () => runMenu('layer-duplicate'),
  e: () => runMenu('merge-down'),
  t: () => runMenu('free-transform'),
  m: () => runMenu('adj-curves'),
  l: () => runMenu('adj-levels'),
  u: () => runMenu('adj-hsl'),
  g: () => runMenu('layer-group'),
  r: () => runMenu('rulers'),
  '=': () => runMenu('zoom-in'),
  '+': () => runMenu('zoom-in'),
  '-': () => runMenu('zoom-out'),
  0: () => runMenu('zoom-fit'),
  1: () => runMenu('zoom-100'),
  2: () => (channel.value = ''),
  3: () => (channel.value = 'r'),
  4: () => (channel.value = 'g'),
  5: () => (channel.value = 'b'),
}

function onKey(event) {
  if (!props.visible || room.value?.offsetParent === null) return
  if (workspace.value || exportOpen.value) return
  const key = event.key.toLowerCase()
  const ctrl = event.ctrlKey || event.metaKey
  if (dialog.value) {
    if (event.key === 'Escape') {
      event.preventDefault()
      closeDialog()
    }
    return
  }
  if (ctrl && !event.altKey && (key === 'o' || key === 's' || key === 'n')) {
    event.preventDefault()
    if (key === 'o') openPicked()
    else if (key === 's') save()
    else if (event.shiftKey) runMenu('layer-new')
    else runMenu('new')
    return
  }
  if (typing(document.activeElement)) return
  if (ctrl) {
    let act
    if (event.code === 'Semicolon') act = event.altKey ? () => runMenu('guides-lock') : event.shiftKey ? () => runMenu('snap') : () => runMenu('guides-show')
    else if (event.code === 'Quote') act = () => runMenu('grid')
    else if (event.altKey && event.shiftKey && key === 'w') act = () => runMenu('export-as')
    else if (event.altKey) act = { i: () => runMenu('image-size'), c: () => runMenu('canvas-size'), r: () => runMenu('select-mask'), g: () => runMenu('layer-clip') }[key]
    else if (event.shiftKey && key === 'i') act = () => runMenu('select-invert')
    else if (event.shiftKey && key === 'g') act = () => runMenu('layer-ungroup')
    else if (event.shiftKey && key === 'x') act = () => runMenu('liquify')
    else if (event.shiftKey && key === 'z') act = redo
    else act = CTRL_KEYS[key]
    if (!act) return
    event.preventDefault()
    act()
    return
  }
  if (event.altKey && event.key === 'Backspace') {
    event.preventDefault()
    runMenu('fill-colour')
    return
  }
  if (event.shiftKey && event.key === 'F5') {
    event.preventDefault()
    runMenu('fill-content')
    return
  }
  if (event.altKey) return
  if (event.shiftKey && CYCLES[key]) {
    const list = CYCLES[key]
    const at = list.indexOf(tool.value)
    tool.value = list[(at + 1) % list.length]
  } else if (key === 'q' && !event.shiftKey) toggleQuickMask()
  else if (key === 'r' && !event.shiftKey) tool.value = 'blurBrush'
  else if (SHORTCUTS[key] && !event.shiftKey) tool.value = SHORTCUTS[key]
  else if (key === 'x') [primary.value, secondary.value] = [secondary.value, primary.value]
  else if (key === '[') brush.size = Math.max(1, Math.round(brush.size * 0.85) - (brush.size < 8 ? 1 : 0))
  else if (key === ']') brush.size = Math.min(500, Math.round(brush.size * 1.18) + (brush.size < 8 ? 1 : 0))
  else return
  event.preventDefault()
}

// ---------- pictures dropped on the room ----------

function onDragDrop(event) {
  if (!props.visible || room.value?.offsetParent === null) return
  const drop = event.payload
  if (drop.type === 'enter' || drop.type === 'over') dropping.value = true
  else if (drop.type === 'leave') dropping.value = false
  else if (drop.type === 'drop') {
    dropping.value = false
    const path = drop.paths?.find((p) => PICTURE.test(p))
    if (path) openPath(path)
    else if (drop.paths?.length) say('That is not a picture PHOTO can open.')
  }
}

watch(
  () => props.visible,
  (on) => {
    if (!on) {
      dropping.value = false
      showRecent.value = false
    }
  },
)

watch(tool, (t) => {
  if (viewMask.value) viewMask.value = ''
  // the quick mask is painted with the brush and the eraser only
  if (quickMask.value && !['brush', 'eraser', 'hand', 'zoom'].includes(t)) toggleQuickMask()
})

onMounted(async () => {
  window.addEventListener('keydown', onKey)
  loadActions()
  try {
    portrait.value = (await avatarGet()) || null
  } catch {
    portrait.value = null
  }
  try {
    stopDrop = await getCurrentWebview().onDragDropEvent(onDragDrop)
  } catch {
    // no drop events on the dev page
  }
  try {
    stopRequest = await onPhotoRequest((request) => {
      // heard here, so the core's copy is cleared too
      photoRequestTake().catch(() => {})
      takeRequest(request)
    })
    takeRequest(await photoRequestTake())
  } catch {
    // outside Castle's window nothing is handed in
  }
})

onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKey)
  stopDrop?.()
  stopRequest?.()
  clearTimeout(noticeTimer)
  clearTimeout(armTimer)
  clearTimeout(previewTimer)
  clearTimeout(histTimer)
  clearTimeout(thumbTimer)
})

defineExpose({ openExternal, openPath })

// keep the dialog's first field ready to type in
watch(dialog, async (d, old) => {
  if (!d || old) return
  await nextTick()
  room.value?.querySelector('.dialog input, .dialog select')?.focus()
})
</script>

<template>
  <section ref="room" class="photo" :class="{ dropping }" aria-label="Photo">
    <header class="head">
      <h1>PHOTO</h1>
    </header>

    <div class="menu-row">
      <MenuBar
        :has-doc="!!doc"
        :can-undo="canUndo"
        :can-redo="canRedo"
        :has-selection="!!doc?.selection"
        :layer-kind="selectedLayer?.kind || ''"
        :has-mask="!!selectedLayer?.mask"
        :can-merge-down="canMergeDown"
        :clipped="!!selectedLayer?.clipped"
        :has-guides="!!doc?.guides?.length"
        :checks="menuChecks"
        :adjustments="ADJ_MENU"
        :filters="filterMenu"
        :artboards="PRESETS"
        :panels="PANELS"
        @run="runMenu"
      />
      <div class="bar">
        <span v-if="recording" class="rec" role="status">
          <span class="rec-dot">●</span> Recording “{{ recording }}” · {{ recSteps.length }}
          <button type="button" class="stop" title="Stop recording" @click="stopRecording">■ Stop</button>
        </span>
        <input v-model="name" class="name" maxlength="60" aria-label="Name to save as" :disabled="!doc" @blur="name = cleanName(name)" @keydown.enter="save" />
        <button type="button" class="primary" :disabled="!doc || busy === 'save'" title="Save (Ctrl+S)" @click="save">{{ busy === 'save' ? 'Saving…' : 'Save' }}</button>
        <span v-if="dirty" class="dot" title="Unsaved changes">&#9679;</span>
        <button v-if="saveBackPath" type="button" class="primary" :disabled="!doc || busy === 'save'" :title="saveBackPath" @click="saveBack">Save back to {{ saveBackName }}</button>
        <span class="sep"></span>
        <button type="button" class="icon" :disabled="!canUndo" title="Undo (Ctrl+Z)" aria-label="Undo" @click="undo">
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9 14 4 9l5-5M4 9h10.5a5.5 5.5 0 0 1 0 11H11" /></svg>
        </button>
        <button type="button" class="icon" :disabled="!canRedo" title="Redo (Ctrl+Y)" aria-label="Redo" @click="redo">
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m15 14 5-5-5-5m5 5H9.5a5.5 5.5 0 0 0 0 11H13" /></svg>
        </button>
      </div>
      <div v-if="showRecent" class="pop recent" role="menu">
        <p v-if="!recent.length" class="dim">Nothing saved yet.</p>
        <button v-for="r in recent" :key="r.name" type="button" class="saved" role="menuitem" :disabled="busy === 'open'" :title="r.savedAt" @click="openRecent(r)">
          <img v-if="r.thumb" :src="r.thumb" alt="" />
          <span v-else class="blank"></span>
          <span>{{ r.name }}</span>
        </button>
        <button type="button" class="close-recent" aria-label="Close" @click="showRecent = false">×</button>
      </div>
    </div>

    <div class="toolcol">
      <ToolColumn class="tools" :tool="tool" :primary="primary" :secondary="secondary" @tool="tool = $event" @colour="onColour" />
      <div class="extras" role="group" aria-label="More tools">
        <button
          v-for="t in EXTRA_TOOLS"
          :key="t.id"
          type="button"
          class="extra"
          :class="{ on: tool === t.id }"
          :aria-pressed="tool === t.id"
          :aria-label="t.label"
          :title="t.label"
          @click="tool = t.id"
        >
          <svg viewBox="0 0 24 24" aria-hidden="true"><path :d="t.path" /></svg>
        </button>
        <button type="button" class="extra" :class="{ on: quickMask, quick: quickMask }" :aria-pressed="quickMask" aria-label="Quick mask (Q)" title="Quick mask (Q)" :disabled="!doc" @click="toggleQuickMask">
          <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="4" y="4" width="16" height="16" rx="2" /><circle cx="12" cy="12" r="4" stroke-dasharray="2 2" /></svg>
        </button>
      </div>
    </div>

    <main class="frame">
      <div class="stage-wrap" :style="{ inset: doc && viewPrefs.rulers ? '20px 0 0 20px' : '0' }">
        <PhotoCanvas
          ref="canvas"
          :companion="companion"
          :doc="doc"
          :selected-id="selectedId"
          :tool="tool"
          :brush="brush"
          :primary="primary"
          :secondary="secondary"
          :tolerance="tolerance"
          :active="visible && !workspace"
          :edit-mask="editMask"
          :view-mask="viewMask"
          :gradient="gradient"
          :shape="shape"
          :snap="viewPrefs.snap"
          :channel="channel"
          :quick-mask="quickMask"
          :active-path="activePath"
          @change="commit"
          @select="selectedId = $event"
          @colour="primary = $event"
          @problem="say"
          @transform="transformT = $event"
          @view="view = $event"
        />
      </div>
      <Rulers
        v-if="doc"
        :view="view"
        :width="doc.width"
        :height="doc.height"
        :guides="doc.guides || []"
        :artboards="doc.artboards || []"
        :grid="doc.grid || {}"
        :rulers="viewPrefs.rulers"
        :show-guides="viewPrefs.guides"
        :movable="tool === 'move' && !viewPrefs.lock"
        @add="onGuideAdd"
        @move="(i, at) => commit(moveGuide(doc, i, at), 'Guide moved')"
        @remove="(i) => commit(removeGuide(doc, i), 'Guide removed')"
      />
      <div v-if="!doc" class="empty">
        <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="4" width="18" height="16" rx="2" /><circle cx="9" cy="10" r="2" /><path d="m4 18 5-5 4 4 3-3 4 4" /></svg>
        <p>Open a photo or a PSD (Ctrl+O), or drop one here.</p>
        <div class="empty-actions">
          <button type="button" class="primary" :disabled="busy === 'open'" @click="openPicked">Open a photo</button>
          <button type="button" @click="runMenu('new')">New…</button>
        </div>
      </div>
      <p v-if="busy === 'open'" class="loading">Opening…</p>
      <p v-else-if="busy === 'work' || busy === 'export'" class="loading">Working…</p>
    </main>

    <aside class="right">
      <Dock ref="dock" :groups="DOCK" :hidden="viewPrefs.hidden">
        <template #layers>
          <LayersPanel
            :layers="doc?.layers || []"
            :selected-id="selectedId"
            :edit-mask="editMask"
            :view-mask="viewMask"
            :blends="BLENDS"
            @select="selectedId = $event"
            @edit-mask="editMask = $event && !!selectedLayer?.mask"
            @view-mask="viewMask = $event"
            @toggle="toggleLayer"
            @rename="(id, n) => commit(renameLayer(doc, id, n), 'Layer renamed')"
            @set="onLayerSet"
            @place="(id, target, mode) => commit(placeLayer(doc, id, target, mode), 'Layer moved')"
            @duplicate="onDuplicate"
            @remove="onRemove"
            @add="addLayer"
            @add-mask="addMask"
            @delete-mask="deleteLayerMask"
            @invert-mask="invertLayerMask"
            @group="onGroup"
            @ungroup="onUngroup"
            @fold="onFold"
            @clip="onClip"
            @style-toggle="(id, key) => commit(toggleStyle(doc, id, key), 'Layer effect shown/hidden')"
            @open-style="openStyle"
          />
        </template>
        <template #channels>
          <ChannelsPanel :thumb="thumb?.pixels || null" :view="channel" @view="channel = $event" @load="loadChannel" />
        </template>
        <template #paths>
          <PathsPanel
            :paths="doc?.paths || []"
            :active-id="activePathId"
            :width="doc?.width || 1"
            :height="doc?.height || 1"
            @pick="activePathId = $event"
            @rename="(id, n) => commit(renamePath(doc, id, n), 'Path renamed')"
            @remove="(id) => commit(removePath(doc, id), 'Path deleted')"
            @stroke="strokePath"
            @fill="fillPath"
            @select="pathSelection"
          />
        </template>
        <template #properties>
          <PropertiesPanel
            docked
            :has-doc="!!doc"
            :layer="selectedLayer"
            :doc-adjust="doc?.adjust || {}"
            :tool="tool"
            :brush="brush"
            :tolerance="tolerance"
            :gradient="gradient"
            :shape="shape"
            :transform="transformT"
            :histogram="histogram"
            :edit-mask="editMask"
            :primary="primary"
            :secondary="secondary"
            :quick-mask="quickMask"
            @adjustment="onAdjustment"
            @adjust="onAdjust"
            @reset-adjust="resetAdjust"
            @brush="Object.assign(brush, $event)"
            @tolerance="tolerance = $event"
            @gradient="gradient = $event"
            @shape="shape = $event"
            @transform="canvas?.setTransform($event)"
            @text="onText"
          />
        </template>
        <template #adjustments>
          <AdjustmentsPanel :disabled="!doc" @add="addAdjustment" />
        </template>
        <template #styles>
          <StylesPanel :disabled="!selectedLayer || selectedLayer.kind === 'adjustment'" @apply="applyStylePreset" />
        </template>
        <template #swatches>
          <SwatchesPanel :primary="primary" @pick="(s) => (s.which === 'secondary' ? (secondary = s.color) : (primary = s.color))" />
        </template>
        <template #actions>
          <ActionsPanel
            :actions="actions"
            :recording="recording"
            :recorded="recSteps.length"
            :busy="actionBusy"
            :has-doc="!!doc"
            @record="startRecording"
            @stop="stopRecording"
            @play="playAction"
            @remove="removeAction"
            @batch="batchAction"
          />
        </template>
        <template #history>
          <HistoryPanel :entries="historyEntries" :index="historyIndex" :companion="companion" @jump="jump" />
        </template>
        <template #navigator>
          <NavigatorPanel :thumb="thumb?.url || ''" :width="doc?.width || 1" :height="doc?.height || 1" :view="view" @center="({ x, y }) => canvas?.centerOn(x, y)" @zoom="(f) => canvas?.zoomBy(f)" />
        </template>
      </Dock>
    </aside>

    <UndoPills class="pills" :pills="pills" @undo="onPillUndo" />

    <div class="chat">
      <p v-if="problem" class="problem" role="alert">
        {{ problem }}
        <button type="button" aria-label="Dismiss" @click="problem = ''">×</button>
      </p>
      <PhotoChat :busy="asking" :lines="sayLines" :portrait="portrait" :companion="companion" @send="onAsk" />
    </div>

    <LayerStyleDialog
      v-if="styleDialog"
      :key="styleDialog.id"
      :layer="styleDialog.before.layers.find((l) => l.id === styleDialog.id)"
      :blends="BLENDS"
      :start="styleDialog.start"
      @preview="previewStyle"
      @ok="okStyle"
      @cancel="cancelStyle"
    />

    <ExportAsDialog v-if="exportOpen && doc" :doc="doc" :title="cleanName(name) || 'image'" :busy="busy === 'export'" @export="runExportAs" @cancel="exportOpen = false" />

    <SelectMaskWorkspace v-if="workspace?.kind === 'mask'" :pixels="workspace.pixels" :mask="workspace.mask" :has-layer="!!selectedLayer && selectedLayer.kind !== 'adjustment'" @done="doneSelectMask" @cancel="workspace = null" />
    <LiquifyWorkspace v-if="workspace?.kind === 'liquify'" :pixels="workspace.pixels" :origin="workspace.origin" :name="workspace.name" @done="doneLiquify" @cancel="workspace = null" />

    <div v-if="dialog" class="dialog-back" @pointerdown.self="closeDialog">
      <form class="dialog" role="dialog" :aria-label="dialog.title" @submit.prevent="applyDialog">
        <h3>{{ dialog.title }}</h3>
        <p v-if="dialog.note" class="dim">{{ dialog.note }}</p>
        <label v-for="f in dialog.fields" :key="f.key" :class="{ check: f.type === 'check' }">
          <span>{{ f.label }}</span>
          <input v-if="f.type === 'check'" type="checkbox" :checked="dialog.values[f.key]" @change="dialogInput(f, $event.target.checked)" />
          <GlassSelect v-else-if="f.type === 'select'" :model-value="dialog.values[f.key]" :options="f.options" :aria-label="f.label" @change="(v) => dialogInput(f, v)" />
          <input v-else-if="f.type === 'number'" type="number" :min="f.min" :max="f.max" :step="f.step" :value="dialog.values[f.key]" @input="dialogInput(f, $event.target.value)" />
          <input v-else-if="f.type === 'text'" type="text" maxlength="60" :value="dialog.values[f.key]" @input="dialogInput(f, $event.target.value)" />
          <template v-else>
            <input type="range" :min="f.min" :max="f.max" :step="f.step" :value="dialog.values[f.key]" @input="dialogInput(f, $event.target.value)" />
            <span class="num">{{ dialog.values[f.key] }}</span>
          </template>
        </label>
        <p v-if="dialog.preview" class="dim">The picture shows it live.</p>
        <div class="dialog-actions">
          <button type="button" @click="closeDialog">Cancel</button>
          <button type="submit" class="primary">{{ dialog.ok || 'OK' }}</button>
        </div>
      </form>
    </div>

    <p v-if="notice" class="notice" role="status">{{ notice }}</p>
  </section>
</template>

<style scoped>
.photo {
  position: absolute;
  inset: 16px;
  display: grid;
  grid-template-columns: 62px minmax(0, 1fr) minmax(300px, 360px);
  grid-template-rows: auto auto minmax(0, 1fr) auto auto;
  grid-template-areas:
    'head head head'
    'menu menu menu'
    'tools frame right'
    'tools pills right'
    'tools chat right';
  gap: 10px 12px;
  padding: 4px 8px 4px;
  color: var(--text, #efe8ff);
}

.head {
  grid-area: head;
}

h1 {
  margin: 0;
  color: var(--accent);
  font-size: 22px;
  font-weight: 600;
  letter-spacing: 0.24em;
}

.menu-row {
  grid-area: menu;
  position: relative;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px 16px;
  padding-bottom: 6px;
  border-bottom: 1px solid var(--line);
}

.bar {
  display: flex;
  flex: 1;
  flex-wrap: wrap;
  align-items: center;
  justify-content: flex-end;
  gap: 6px;
}

.bar > button,
.empty button,
.dialog-actions button {
  padding: 5px 14px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: none;
  color: var(--text);
  font-size: 12.5px;
}

.bar button.icon {
  display: grid;
  place-items: center;
  width: 32px;
  height: 30px;
  padding: 0;
}

.icon svg,
.empty svg {
  width: 18px;
  height: 18px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.6;
  stroke-linecap: round;
  stroke-linejoin: round;
}

button.primary {
  border-color: var(--accent);
  background: var(--accent);
  color: #160f33;
}

.rec {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 3px 4px 3px 10px;
  border: 1px solid color-mix(in srgb, var(--bad, #ff6b8b) 55%, transparent);
  border-radius: 999px;
  color: var(--text);
  font-size: 12px;
}

.rec-dot {
  color: var(--bad, #ff6b8b);
  animation: blink 1.2s ease-in-out infinite;
}

@keyframes blink {
  50% {
    opacity: 0.3;
  }
}

@media (prefers-reduced-motion: reduce) {
  .rec-dot {
    animation: none;
  }
}

.rec .stop {
  padding: 2px 10px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: none;
  color: var(--text);
  font-size: 11.5px;
}

.sep {
  width: 1px;
  height: 20px;
  margin: 0 4px;
  background: var(--line);
}

.name {
  width: 160px;
  padding: 5px 10px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: rgba(0, 0, 0, 0.2);
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
}

.dot {
  color: var(--accent);
  font-size: 10px;
}

.pop {
  position: absolute;
  top: calc(100% + 6px);
  z-index: 35;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: var(--panel-background);
  backdrop-filter: blur(var(--glass-blur));
  box-shadow: 0 10px 30px rgba(0, 0, 0, 0.45);
}

.recent {
  left: 0;
  display: flex;
  gap: 8px;
  width: max-content;
  max-width: min(680px, 70vw);
  padding: 10px 30px 10px 10px;
  overflow-x: auto;
}

.close-recent {
  position: absolute;
  top: 4px;
  right: 6px;
  border: 0;
  background: none;
  color: var(--text-dim);
  font-size: 16px;
}

.saved {
  display: flex;
  flex: none;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  width: 116px;
  padding: 6px;
  border: 1px solid transparent;
  border-radius: 10px;
  background: none;
  color: var(--text);
}

.saved:hover {
  border-color: var(--line);
}

.saved img,
.saved .blank {
  width: 100%;
  height: 70px;
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.08);
  object-fit: cover;
}

.saved span:last-child {
  max-width: 100%;
  overflow: hidden;
  font-size: 11.5px;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.num {
  min-width: 34px;
  color: var(--text-dim);
  font-variant-numeric: tabular-nums;
  text-align: right;
}

.toolcol {
  grid-area: tools;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
  overflow-y: auto;
  scrollbar-width: none;
}

.tools {
  flex: none;
  overflow: visible;
}

.extras {
  display: grid;
  flex: none;
  grid-template-columns: 1fr 1fr;
  gap: 3px;
  padding: 8px 5px;
  border: 1px solid var(--line, rgba(190, 170, 255, 0.3));
  border-radius: 16px;
  background: var(--panel-background, rgba(24, 19, 66, 0.72));
  backdrop-filter: blur(var(--glass-blur, 8px));
}

.extra {
  display: grid;
  place-items: center;
  height: 28px;
  padding: 0;
  border: 1px solid transparent;
  border-radius: 8px;
  background: none;
  color: var(--accent, #a98bff);
}

.extra svg {
  width: 18px;
  height: 18px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.6;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.extra:hover:not(.on):not(:disabled) {
  border-color: var(--line);
}

.extra.on {
  border-color: color-mix(in srgb, var(--accent, #8a5cf0) 50%, transparent);
  background: color-mix(in srgb, var(--accent, #8a5cf0) 28%, transparent);
  color: var(--text, #fff);
}

.extra.quick {
  border-color: rgba(255, 40, 60, 0.6);
  color: #ff8a9a;
}

.frame {
  grid-area: frame;
  position: relative;
  min-width: 0;
  min-height: 0;
  overflow: hidden;
  border: 1px solid var(--line, rgba(190, 170, 255, 0.3));
  border-radius: 16px;
  background: color-mix(in srgb, var(--panel-background, rgba(24, 19, 66, 0.72)) 70%, transparent);
}

.stage-wrap {
  position: absolute;
}

.photo.dropping .frame {
  outline: 2px dashed var(--accent);
  outline-offset: 2px;
}

.empty {
  position: absolute;
  inset: 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 12px;
  color: var(--text-dim);
  pointer-events: none;
}

.empty svg {
  width: 54px;
  height: 54px;
  stroke: var(--accent);
  opacity: 0.8;
}

.empty p {
  margin: 0;
  font-size: 15px;
}

.empty-actions {
  display: flex;
  gap: 8px;
  pointer-events: auto;
}

.loading {
  position: absolute;
  top: 30px;
  left: 50%;
  z-index: 8;
  margin: 0;
  padding: 4px 12px;
  border-radius: 999px;
  background: var(--panel-background, rgba(24, 19, 66, 0.9));
  color: var(--text-dim);
  font-size: 12px;
  transform: translateX(-50%);
}

.right {
  grid-area: right;
  display: flex;
  flex-direction: column;
  min-height: 0;
}

/* inside the dock HISTORY's own title is the tab */
.right :deep(.history > header) {
  display: none;
}

.pills {
  grid-area: pills;
  min-width: 0;
}

.chat {
  grid-area: chat;
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}

.problem {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0;
  padding: 6px 12px;
  border: 1px solid color-mix(in srgb, var(--bad, #ff6b8b) 50%, transparent);
  border-radius: 10px;
  background: color-mix(in srgb, var(--bad, #ff6b8b) 12%, transparent);
  color: var(--bad, #ff6b8b);
  font-size: 12.5px;
}

.problem button {
  margin-left: auto;
  padding: 0 4px;
  border: 0;
  background: none;
  color: inherit;
  font-size: 16px;
}

.dialog-back {
  position: absolute;
  inset: 0;
  z-index: 50;
  display: grid;
  place-items: start center;
  padding-top: 90px;
  background: rgba(6, 3, 20, 0.35);
}

.dialog {
  display: flex;
  flex-direction: column;
  gap: 10px;
  width: min(380px, 90vw);
  padding: 16px 18px;
  border: 1px solid var(--line);
  border-radius: 16px;
  background: var(--panel-background);
  backdrop-filter: blur(var(--glass-blur));
  box-shadow: 0 16px 40px rgba(0, 0, 0, 0.55);
}

.dialog h3 {
  margin: 0;
  color: var(--accent);
  font-size: 15px;
  font-weight: 600;
  letter-spacing: 0.08em;
}

.dialog label {
  display: flex;
  align-items: center;
  gap: 10px;
  color: var(--text-dim);
  font-size: 12.5px;
}

.dialog label > span:first-child {
  width: 110px;
}

.dialog input[type='range'] {
  flex: 1;
  accent-color: var(--accent);
}

.dialog input[type='number'],
.dialog input[type='text'],
.dialog select {
  flex: 1;
  padding: 4px 8px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
}

.dialog input[type='checkbox'] {
  accent-color: var(--accent);
}

.dialog-actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

.dim {
  margin: 0;
  color: var(--text-dim);
  font-size: 12px;
}

button {
  font: inherit;
  cursor: pointer;
}

button:disabled {
  opacity: 0.45;
  cursor: default;
}

button:focus-visible,
input:focus-visible,
select:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

.notice {
  position: absolute;
  left: 50%;
  bottom: 120px;
  z-index: 65;
  margin: 0;
  padding: 8px 16px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: rgba(30, 20, 60, 0.94);
  color: #fff;
  font-size: 13px;
  transform: translateX(-50%);
}
</style>
