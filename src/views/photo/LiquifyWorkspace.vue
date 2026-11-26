<script setup>
import { computed, onBeforeUnmount, onMounted, reactive, ref } from 'vue'
import { applyMesh, liquifyMesh, liquifyStroke } from './engine3.js'

// PHOTO v3's Liquify: the whole room becomes the workspace for one picture layer. Push drags the pixels
// along the brush, bloat swells them, pucker pinches them in, reconstruct paints them back. The brush has a
// size and a pressure (a pen's own pressure counts too). The preview runs through the same mesh at a light
// size; OK gives the mesh and the strokes back so the view warps the real pixels (and keeps the strokes as
// her liquify op for actions), Cancel leaves the layer as it was.
const props = defineProps({
  pixels: { type: Object, required: true }, // the layer's own pixels
  origin: { type: Object, default: () => ({ x: 0, y: 0 }) }, // where the layer sits in the document
  name: { type: String, default: 'Layer' },
})
const emit = defineEmits(['done', 'cancel'])

const MAX = 0.6e6
const W = props.pixels.width
const H = props.pixels.height
const s = Math.min(1, Math.sqrt(MAX / (W * H)))
const w = Math.max(1, Math.round(W * s))
const h = Math.max(1, Math.round(H * s))
const TOOLS = [
  { id: 'push', label: 'Forward warp (push)', key: 'W', path: 'M4 12h12m-4-5 5 5-5 5M4 6c3 0 5 2 6 6' },
  { id: 'reconstruct', label: 'Reconstruct', key: 'R', path: 'M4 4v5h5M4.6 9A8 8 0 1 1 6 16.5' },
  { id: 'pucker', label: 'Pucker', key: 'S', path: 'M12 4v5m0 6v5M4 12h5m6 0h5M7 7l3 3m4 4 3 3M17 7l-3 3m-4 4-3 3' },
  { id: 'bloat', label: 'Bloat', key: 'B', path: 'M12 9V3m0 12v6M9 12H3m12 0h6M10 10 6 6m8 8 4 4m-4-8 4-4m-8 8-4 4' },
]

const tool = ref('push')
const brush = reactive({ size: 120, pressure: 50, usePen: true })
const out = ref(null)
const hover = ref(null)
const working = ref(false)
const strokes = [] // doc-space strokes, as her liquify op keeps them

let mesh = liquifyMesh(W, H)
let small = null
let current = null
let raf = 0

function makeCanvas(cw, ch) {
  const c = document.createElement('canvas')
  c.width = cw
  c.height = ch
  return c
}

function setup() {
  const src = makeCanvas(W, H)
  src.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(props.pixels.data), W, H), 0, 0)
  const c = makeCanvas(w, h)
  const ctx = c.getContext('2d', { willReadFrequently: true })
  ctx.imageSmoothingQuality = 'high'
  ctx.drawImage(src, 0, 0, w, h)
  const d = ctx.getImageData(0, 0, w, h)
  small = { data: d.data, width: w, height: h }
}

function draw() {
  raf = 0
  if (!out.value || !small) return
  const p = applyMesh(small, mesh)
  out.value.getContext('2d').putImageData(new ImageData(new Uint8ClampedArray(p.data), w, h), 0, 0)
}

const redraw = () => !raf && (raf = requestAnimationFrame(draw))

function point(event) {
  const r = out.value.getBoundingClientRect()
  return { x: ((event.clientX - r.left) / r.width) * W, y: ((event.clientY - r.top) / r.height) * H, scr: r.width / W }
}

function pressureOf(event) {
  const base = brush.pressure / 100
  return brush.usePen && event.pointerType === 'pen' && event.pressure > 0 ? Math.max(0.01, base * event.pressure * 2) : base
}

function down(event) {
  if (event.button !== 0) return
  event.currentTarget.setPointerCapture?.(event.pointerId)
  const p = point(event)
  current = { mode: tool.value, size: brush.size, pressure: pressureOf(event), points: [[p.x, p.y]] }
  mesh = liquifyStroke(mesh, { ...current, points: [[p.x, p.y]] })
  redraw()
}

function move(event) {
  const p = point(event)
  hover.value = p
  if (!current) return
  const last = current.points.at(-1)
  if (Math.hypot(p.x - last[0], p.y - last[1]) < Math.max(2, brush.size / 10)) return
  mesh = liquifyStroke(mesh, { mode: current.mode, size: current.size, pressure: pressureOf(event), points: [last, [p.x, p.y]] })
  current.points.push([p.x, p.y])
  redraw()
}

function up() {
  if (!current) return
  const c = current
  current = null
  // kept in document space, at most 500 points a stroke (her op's limit)
  const step = Math.max(1, Math.ceil(c.points.length / 500))
  strokes.push({ mode: c.mode, size: Math.round(c.size), pressure: Math.round(c.pressure * 100) / 100, points: c.points.filter((_, i) => i % step === 0).map(([x, y]) => [Math.round(x + props.origin.x), Math.round(y + props.origin.y)]) })
}

function restoreAll() {
  mesh = liquifyMesh(W, H)
  strokes.length = 0
  redraw()
}

const ring = computed(() => {
  const p = hover.value
  if (!p) return null
  const d = brush.size * p.scr
  return { left: `${(p.x / W) * 100}%`, top: `${(p.y / H) * 100}%`, width: `${d}px`, height: `${d}px` }
})

async function ok() {
  if (!strokes.length) return emit('cancel')
  working.value = true
  await new Promise((r) => setTimeout(r, 30))
  try {
    emit('done', { mesh, strokes: strokes.slice(0, 50) })
  } finally {
    working.value = false
  }
}

function onKey(event) {
  const t = TOOLS.find((x) => x.key.toLowerCase() === event.key.toLowerCase())
  if (event.key === 'Escape') emit('cancel')
  else if (event.key === 'Enter') ok()
  else if (event.key === '[' || event.key === ']') brush.size = Math.max(4, Math.min(1500, Math.round(brush.size * (event.key === '[' ? 0.85 : 1.18))))
  else if (t && !event.ctrlKey) tool.value = t.id
  else return
  event.preventDefault()
  event.stopPropagation()
}

onMounted(() => {
  setup()
  window.addEventListener('keydown', onKey, true)
  redraw()
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onKey, true)
  cancelAnimationFrame(raf)
})
</script>

<template>
  <section class="workspace" role="dialog" aria-label="Liquify">
    <header>
      <h2>LIQUIFY</h2>
      <span class="dim">{{ name }} · W push, R reconstruct, S pucker, B bloat · [ ] brush size · Enter keeps · Esc cancels</span>
    </header>
    <aside class="tools" aria-label="Liquify tools">
      <button v-for="t in TOOLS" :key="t.id" type="button" :class="{ on: tool === t.id }" :aria-pressed="tool === t.id" :title="`${t.label} (${t.key})`" @click="tool = t.id">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path :d="t.path" /></svg>
      </button>
    </aside>
    <main class="view" @pointerleave="hover = null">
      <div class="fit" :style="{ aspectRatio: `${w} / ${h}`, width: `min(100cqw, calc(100cqh * ${w / h}))` }">
        <canvas ref="out" :width="w" :height="h" @pointerdown="down" @pointermove="move" @pointerup="up" @pointercancel="up"></canvas>
        <span v-if="ring" class="ring" :style="ring"></span>
      </div>
      <p v-if="working" class="working">Working…</p>
    </main>
    <aside class="props">
      <h3>Brush</h3>
      <label class="slide"><span>Size</span><input v-model.number="brush.size" type="range" min="4" max="1500" /><span class="num">{{ brush.size }}px</span></label>
      <label class="slide"><span>Pressure</span><input v-model.number="brush.pressure" type="range" min="1" max="100" /><span class="num">{{ brush.pressure }}%</span></label>
      <label class="check"><input v-model="brush.usePen" type="checkbox" /> Pen pressure counts</label>
      <button type="button" class="wide" @click="restoreAll">Restore all</button>
      <p class="dim">The face tools are not part of this Liquify.</p>
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
  grid-template-columns: 56px minmax(0, 1fr) 250px;
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
  cursor: pointer;
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
  background: repeating-conic-gradient(#d9d6e4 0 25%, #ffffff 0 50%) 0 0 / 16px 16px;
  cursor: none;
  touch-action: none;
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
}

.slide {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text-dim);
  font-size: 12px;
}

.slide span:first-child {
  width: 64px;
}

.slide input {
  flex: 1;
  min-width: 40px;
  accent-color: var(--accent);
}

.check {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--text-dim);
  font-size: 12px;
}

.check input {
  accent-color: var(--accent);
}

.num {
  width: 50px;
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.wide,
.actions button {
  padding: 6px 14px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.actions {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
  margin-top: auto;
}

.actions button {
  min-width: 80px;
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
  margin: 0;
  color: var(--text-dim);
  font-size: 12px;
}

button:focus-visible,
input:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
</style>
