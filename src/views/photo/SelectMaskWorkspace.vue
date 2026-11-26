<script setup>
import { computed, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import GlassSelect from '../../components/GlassSelect.vue'
import { refineMask } from './engine2.js'

// PHOTO v3's Select and Mask: the whole room becomes the workspace. The selection shows over the picture in
// a view he picks (onion skin, overlay, on black, on white, black & white); the brush adds to it (Alt takes
// away), the refine-edge brush lets the edge find itself where it paints (hair, fur); smooth, feather,
// contrast and shift edge work on the whole edge. Output to a selection, a layer mask, or a new masked layer.
// It works at a light size and gives back a full-size mask.
const props = defineProps({
  pixels: { type: Object, required: true }, // the picture, document sized
  mask: { type: Object, default: null }, // Uint8Array(w*h) - the selection to start from
  hasLayer: { type: Boolean, default: false },
})
const emit = defineEmits(['done', 'cancel'])

const MAX = 1.2e6
const W = props.pixels.width
const H = props.pixels.height
const s = Math.min(1, Math.sqrt(MAX / (W * H)))
const w = Math.max(1, Math.round(W * s))
const h = Math.max(1, Math.round(H * s))

const view = ref('onion')
const tool = ref('brush')
const values = reactive({ size: 40, smooth: 0, feather: 0, contrast: 0, shift: 0, onion: 50 })
const output = ref('selection')
const out = ref(null)
const hover = ref(null)
const working = ref(false)

let picture = null // preview pixels
let base = null // the edited mask at preview size
let shown = null // after refine
let stroke = null
let raf = 0
let timer = 0

function makeCanvas(cw, ch) {
  const c = document.createElement('canvas')
  c.width = cw
  c.height = ch
  return c
}

function toCanvas(p) {
  const c = makeCanvas(p.width, p.height)
  c.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(p.data), p.width, p.height), 0, 0)
  return c
}

function maskToCanvas(m, mw, mh) {
  const c = makeCanvas(mw, mh)
  const ctx = c.getContext('2d')
  const img = ctx.createImageData(mw, mh)
  for (let j = 0; j < m.length; j++) {
    const i = j * 4
    img.data[i] = img.data[i + 1] = img.data[i + 2] = m[j]
    img.data[i + 3] = 255
  }
  ctx.putImageData(img, 0, 0)
  return c
}

function resizeMask(m, mw, mh, tw, th) {
  if (mw === tw && mh === th) return Uint8Array.from(m)
  const c = makeCanvas(tw, th)
  const ctx = c.getContext('2d', { willReadFrequently: true })
  ctx.imageSmoothingQuality = 'high'
  ctx.drawImage(maskToCanvas(m, mw, mh), 0, 0, tw, th)
  const d = ctx.getImageData(0, 0, tw, th).data
  const o = new Uint8Array(tw * th)
  for (let j = 0; j < o.length; j++) o[j] = d[j * 4]
  return o
}

function setup() {
  const c = makeCanvas(w, h)
  const ctx = c.getContext('2d', { willReadFrequently: true })
  ctx.imageSmoothingQuality = 'high'
  ctx.drawImage(toCanvas(props.pixels), 0, 0, w, h)
  const d = ctx.getImageData(0, 0, w, h)
  picture = { data: d.data, width: w, height: h }
  base = props.mask ? resizeMask(props.mask, W, H, w, h) : new Uint8Array(w * h)
}

function contrastOf(m, c) {
  if (!c) return m
  const k = 1 + c / 20
  const o = new Uint8Array(m.length)
  for (let j = 0; j < m.length; j++) o[j] = Math.max(0, Math.min(255, (m[j] - 128) * k + 128))
  return o
}

function refined(m, mw, mh, scale) {
  const r = values.smooth || values.feather || values.shift ? refineMask(m, mw, mh, { smooth: values.smooth * scale, feather: values.feather * scale, shift: Math.round(values.shift * scale) }) : m
  return contrastOf(r, values.contrast)
}

function draw() {
  raf = 0
  if (!out.value || !picture) return
  shown = refined(base, w, h, s)
  const ctx = out.value.getContext('2d')
  const img = ctx.createImageData(w, h)
  const p = picture.data
  const d = img.data
  const onion = values.onion / 100
  for (let j = 0, i = 0; j < shown.length; j++, i += 4) {
    const m = shown[j] / 255
    if (view.value === 'bw') {
      d[i] = d[i + 1] = d[i + 2] = shown[j]
      d[i + 3] = 255
      continue
    }
    if (view.value === 'overlay') {
      const t = (1 - m) * 0.5
      d[i] = p[i] + (255 - p[i]) * t
      d[i + 1] = p[i + 1] * (1 - t)
      d[i + 2] = p[i + 2] * (1 - t)
      d[i + 3] = 255
      continue
    }
    const back = view.value === 'white' ? 255 : view.value === 'black' ? 0 : -1
    if (back >= 0) {
      d[i] = p[i] * m + back * (1 - m)
      d[i + 1] = p[i + 1] * m + back * (1 - m)
      d[i + 2] = p[i + 2] * m + back * (1 - m)
      d[i + 3] = 255
    } else {
      d[i] = p[i]
      d[i + 1] = p[i + 1]
      d[i + 2] = p[i + 2]
      d[i + 3] = 255 * (m + (1 - m) * (1 - onion))
    }
  }
  ctx.putImageData(img, 0, 0)
}

function redraw(soon = false) {
  if (soon) {
    clearTimeout(timer)
    timer = setTimeout(() => !raf && (raf = requestAnimationFrame(draw)), 60)
  } else if (!raf) raf = requestAnimationFrame(draw)
}

watch(() => [view.value, values.onion], () => redraw())
watch(() => [values.smooth, values.feather, values.contrast, values.shift], () => redraw(true))

// ---------- the brushes ----------

function point(event) {
  const r = out.value.getBoundingClientRect()
  return { x: ((event.clientX - r.left) / r.width) * w, y: ((event.clientY - r.top) / r.height) * h, scr: r.width / w }
}

function dab(p, add) {
  const r = Math.max(1, (values.size * s) / 2)
  const x0 = Math.max(0, Math.floor(p.x - r))
  const x1 = Math.min(w, Math.ceil(p.x + r))
  const y0 = Math.max(0, Math.floor(p.y - r))
  const y1 = Math.min(h, Math.ceil(p.y + r))
  for (let y = y0; y < y1; y++) {
    for (let x = x0; x < x1; x++) {
      const d = Math.hypot(x + 0.5 - p.x, y + 0.5 - p.y)
      if (d >= r) continue
      const j = y * w + x
      const c = d < r * 0.7 ? 1 : 1 - (d - r * 0.7) / (r * 0.3)
      base[j] = add ? Math.max(base[j], 255 * c) : Math.min(base[j], 255 * (1 - c))
    }
  }
}

// Refine edge: inside the brush, each pixel's selection follows how close its colour is to the sure inside
// (mask > 220) or the sure outside (mask < 35) around the brush.
function refineDab(p) {
  const r = Math.max(2, (values.size * s) / 2)
  const R = r * 2
  const x0 = Math.max(0, Math.floor(p.x - R))
  const x1 = Math.min(w, Math.ceil(p.x + R))
  const y0 = Math.max(0, Math.floor(p.y - R))
  const y1 = Math.min(h, Math.ceil(p.y + R))
  const fg = [0, 0, 0, 0]
  const bg = [0, 0, 0, 0]
  const d = picture.data
  for (let y = y0; y < y1; y += 2) {
    for (let x = x0; x < x1; x += 2) {
      const j = y * w + x
      const v = base[j]
      const t = v > 220 ? fg : v < 35 ? bg : null
      if (!t) continue
      t[0] += d[j * 4]
      t[1] += d[j * 4 + 1]
      t[2] += d[j * 4 + 2]
      t[3] += 1
    }
  }
  if (!fg[3] || !bg[3]) return
  for (const t of [fg, bg]) for (let k = 0; k < 3; k++) t[k] /= t[3]
  for (let y = Math.max(0, Math.floor(p.y - r)); y < Math.min(h, Math.ceil(p.y + r)); y++) {
    for (let x = Math.max(0, Math.floor(p.x - r)); x < Math.min(w, Math.ceil(p.x + r)); x++) {
      if (Math.hypot(x + 0.5 - p.x, y + 0.5 - p.y) >= r) continue
      const j = y * w + x
      const i = j * 4
      const df = Math.hypot(d[i] - fg[0], d[i + 1] - fg[1], d[i + 2] - fg[2])
      const db = Math.hypot(d[i] - bg[0], d[i + 1] - bg[1], d[i + 2] - bg[2])
      const a = db / Math.max(1, df + db)
      base[j] = Math.round(255 * Math.max(0, Math.min(1, (a - 0.5) * 2.2 + 0.5)))
    }
  }
}

function strokeAt(p) {
  const step = Math.max(1, values.size * s * 0.2)
  const last = stroke.last || p
  const dist = Math.hypot(p.x - last.x, p.y - last.y)
  const n = Math.max(1, Math.ceil(dist / step))
  for (let k = stroke.last ? 1 : 0; k <= n; k++) {
    const q = { x: last.x + ((p.x - last.x) * k) / n, y: last.y + ((p.y - last.y) * k) / n }
    if (tool.value === 'refine') refineDab(q)
    else dab(q, !stroke.alt)
  }
  stroke.last = p
  redraw()
}

function down(event) {
  if (event.button !== 0) return
  event.currentTarget.setPointerCapture?.(event.pointerId)
  stroke = { last: null, alt: event.altKey }
  strokeAt(point(event))
}

function move(event) {
  const p = point(event)
  hover.value = p
  if (stroke) strokeAt(p)
}

function up() {
  stroke = null
}

const ring = computed(() => {
  const p = hover.value
  if (!p) return null
  const d = values.size * s * p.scr
  return { left: `${(p.x / w) * 100}%`, top: `${(p.y / h) * 100}%`, width: `${d}px`, height: `${d}px` }
})

async function ok() {
  working.value = true
  await new Promise((r) => setTimeout(r, 30))
  try {
    const full = s < 1 ? resizeMask(base, w, h, W, H) : Uint8Array.from(base)
    emit('done', { mask: refined(full, W, H, 1), output: output.value })
  } finally {
    working.value = false
  }
}

function onKey(event) {
  if (event.key === 'Escape') {
    event.preventDefault()
    event.stopPropagation()
    emit('cancel')
  } else if (event.key === 'Enter' && event.target.tagName !== 'SELECT') {
    event.preventDefault()
    event.stopPropagation()
    ok()
  } else if (event.key === '[' || event.key === ']') {
    values.size = Math.max(2, Math.min(500, Math.round(values.size * (event.key === '[' ? 0.85 : 1.18))))
    event.stopPropagation()
  }
}

onMounted(() => {
  setup()
  window.addEventListener('keydown', onKey, true)
  redraw()
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKey, true)
  cancelAnimationFrame(raf)
  clearTimeout(timer)
})

const VIEWS = [
  ['onion', 'Onion skin'],
  ['overlay', 'Overlay'],
  ['black', 'On black'],
  ['white', 'On white'],
  ['bw', 'Black & white'],
]
</script>

<template>
  <section class="workspace" role="dialog" aria-label="Select and Mask">
    <header>
      <h2>SELECT AND MASK</h2>
      <span class="dim">Paint to add (Alt takes away) · [ ] brush size · Enter keeps · Esc cancels</span>
    </header>
    <aside class="tools" aria-label="Brushes">
      <button type="button" :class="{ on: tool === 'brush' }" :aria-pressed="tool === 'brush'" title="Brush: paint the selection (Alt takes away)" @click="tool = 'brush'">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M20 4c-3 1.5-7.5 6-9.4 8.6l1.8 1.8C15 12.5 19 8 20 4ZM9.6 13.6c-2 .1-3.2 1.3-3.4 3.2-.1 1.3-.8 2.2-2.2 2.7 2.8 1 6.6.3 7.4-2.4l-1.8-3.5Z" /></svg>
      </button>
      <button type="button" :class="{ on: tool === 'refine' }" :aria-pressed="tool === 'refine'" title="Refine edge brush: paint over hair and soft edges" @click="tool = 'refine'">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 19c3-1 4-4 4-7m0 0c0-3 2-5 4-6m-4 6c2 0 4 1 5 3m-5-3c-1-2-3-3-5-3M13 6c2-1 4-1 6 0" /></svg>
      </button>
    </aside>
    <main class="view" @pointerleave="hover = null">
      <div class="fit" :style="{ aspectRatio: `${w} / ${h}`, width: `min(100cqw, calc(100cqh * ${w / h}))` }">
        <canvas ref="out" :width="w" :height="h" :class="{ checker: view === 'onion' }" @pointerdown="down" @pointermove="move" @pointerup="up" @pointercancel="up"></canvas>
        <span v-if="ring" class="ring" :style="ring"></span>
      </div>
      <p v-if="working" class="working">Working…</p>
    </main>
    <aside class="props">
      <h3>View</h3>
      <GlassSelect v-model="view" aria-label="View mode" :options="VIEWS.map(([id, label]) => ({ value: id, label }))" />
      <label v-if="view === 'onion'" class="slide"><span>Transparency</span><input v-model.number="values.onion" type="range" min="0" max="100" /><span class="num">{{ values.onion }}%</span></label>
      <h3>Brush</h3>
      <label class="slide"><span>Size</span><input v-model.number="values.size" type="range" min="2" max="500" /><span class="num">{{ values.size }}px</span></label>
      <h3>Global refinements</h3>
      <label class="slide"><span>Smooth</span><input v-model.number="values.smooth" type="range" min="0" max="20" /><span class="num">{{ values.smooth }}</span></label>
      <label class="slide"><span>Feather</span><input v-model.number="values.feather" type="range" min="0" max="60" step="0.5" /><span class="num">{{ values.feather }}px</span></label>
      <label class="slide"><span>Contrast</span><input v-model.number="values.contrast" type="range" min="0" max="100" /><span class="num">{{ values.contrast }}%</span></label>
      <label class="slide"><span>Shift edge</span><input v-model.number="values.shift" type="range" min="-30" max="30" /><span class="num">{{ values.shift }}</span></label>
      <h3>Output to</h3>
      <GlassSelect
        v-model="output"
        aria-label="Output"
        :options="[{ value: 'selection', label: 'Selection' }, { value: 'mask', label: 'Layer mask', disabled: !hasLayer }, { value: 'layer', label: 'New layer with a mask', disabled: !hasLayer }]"
      />
      <div class="actions">
        <button type="button" @click="emit('cancel')">Cancel</button>
        <button type="button" class="primary" :disabled="working" @click="ok">OK</button>
      </div>
    </aside>
  </section>
</template>

<style scoped>
.workspace {
  position: absolute;
  inset: 0;
  z-index: 60;
  display: grid;
  grid-template-columns: 56px minmax(0, 1fr) 270px;
  grid-template-rows: auto minmax(0, 1fr);
  grid-template-areas: 'head head head' 'tools view props';
  gap: 10px;
  padding: 10px;
  border-radius: 16px;
  background: color-mix(in srgb, var(--panel-background) 96%, #000);
  color: var(--text);
}

header {
  grid-area: head;
  display: flex;
  align-items: baseline;
  gap: 16px;
}

h2 {
  margin: 0;
  color: var(--accent);
  font-size: 15px;
  font-weight: 600;
  letter-spacing: 0.2em;
}

h3 {
  margin: 6px 0 0;
  color: var(--text-dim);
  font-size: 11.5px;
  font-weight: 500;
  letter-spacing: 0.14em;
  text-transform: uppercase;
}

.tools {
  grid-area: tools;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.tools button {
  display: grid;
  place-items: center;
  width: 44px;
  height: 40px;
  padding: 0;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: none;
  color: var(--accent);
}

.tools button.on {
  background: color-mix(in srgb, var(--accent) 28%, transparent);
  color: var(--text);
}

svg {
  width: 22px;
  height: 22px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.6;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.view {
  grid-area: view;
  position: relative;
  display: grid;
  place-items: center;
  min-height: 0;
  overflow: hidden;
  border: 1px solid var(--line);
  border-radius: 12px;
  container-type: size;
}

.fit {
  position: relative;
}

canvas {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: contain;
  cursor: none;
  touch-action: none;
}

canvas.checker {
  background: repeating-conic-gradient(#d9d6e4 0 25%, #ffffff 0 50%) 0 0 / 16px 16px;
}

.ring {
  position: absolute;
  border: 1px solid #fff;
  border-radius: 50%;
  box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.6);
  transform: translate(-50%, -50%);
  pointer-events: none;
}

.props {
  grid-area: props;
  display: flex;
  flex-direction: column;
  gap: 8px;
  overflow-y: auto;
}

.slide {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text-dim);
  font-size: 12px;
}

.slide span:first-child {
  width: 80px;
}

.slide input {
  flex: 1;
  min-width: 40px;
  accent-color: var(--accent);
}

.num {
  width: 44px;
  text-align: right;
  font-variant-numeric: tabular-nums;
}

select {
  padding: 4px 8px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
}

.actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: auto;
  padding-top: 10px;
}

.actions button {
  min-width: 80px;
  padding: 6px 14px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.actions button.primary {
  border-color: var(--accent);
  background: var(--accent);
  color: #160f33;
}

.working {
  position: absolute;
  top: 10px;
  left: 50%;
  margin: 0;
  padding: 4px 12px;
  border-radius: 999px;
  background: var(--panel-background, rgba(24, 19, 66, 0.9));
  color: var(--text-dim);
  font-size: 12px;
  transform: translateX(-50%);
}

.dim {
  color: var(--text-dim);
  font-size: 12px;
}

button:focus-visible,
select:focus-visible,
input:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
</style>
