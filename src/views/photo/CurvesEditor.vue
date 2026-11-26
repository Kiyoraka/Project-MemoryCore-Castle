<script setup>
import { computed, ref } from 'vue'
import { curvesLut } from './engine2.js'
import { CURVE_CHANNELS, insertCurvePoint, moveCurvePoint, removeCurvePoint } from './viewHelpers.js'

// PHOTO's curves: one graph per channel (RGB, R, G, B) over the histogram. A click adds a point,
// a drag moves it, dragging it out of the graph takes it away.
const props = defineProps({
  params: { type: Object, default: () => ({ rgb: [[0, 0], [255, 255]] }) },
  histogram: { type: Object, default: null },
})
const emit = defineEmits(['update'])

const NAMES = { rgb: 'RGB', r: 'Red', g: 'Green', b: 'Blue' }
const COLOURS = { rgb: 'var(--accent, #a98bff)', r: '#ff6b6b', g: '#5be38a', b: '#5aa8ff' }
const IDENTITY = [[0, 0], [255, 255]]

const channel = ref('rgb')
const graph = ref(null)
const dragging = ref(null) // { index, out }

const points = computed(() => props.params?.[channel.value] || IDENTITY)

const curvePath = computed(() => {
  let lut
  try {
    lut = curvesLut(points.value)
  } catch {
    lut = null
  }
  if (!lut) return `M${points.value.map((p) => `${p[0]} ${255 - p[1]}`).join('L')}`
  let d = ''
  for (let x = 0; x < 256; x += 1) d += `${x ? 'L' : 'M'}${x} ${255 - lut[x]}`
  return d
})

const histPath = computed(() => {
  const h = props.histogram
  const bins = h ? (channel.value === 'rgb' ? h.luma : h[channel.value]) : null
  if (!bins?.length) return ''
  let max = 0
  for (let i = 1; i < 255; i++) if (bins[i] > max) max = bins[i]
  if (!max) return ''
  let d = 'M0 256'
  for (let i = 0; i < 256; i++) d += `L${i} ${256 - Math.min(1, bins[i] / max) * 230}`
  return `${d}L255 256Z`
})

function toGraph(event) {
  const r = graph.value.getBoundingClientRect()
  // the viewBox runs from -4 to 260 so the points at the ends are not cut in half
  return { x: -4 + ((event.clientX - r.left) / r.width) * 264, y: 255 - (-4 + ((event.clientY - r.top) / r.height) * 264) }
}

function send(list, final) {
  emit('update', { ...props.params, [channel.value]: list }, final)
}

function onDown(event) {
  if (event.button !== 0) return
  const p = toGraph(event)
  const list = points.value
  // near an existing point: drag it; anywhere else: a new point
  let index = list.findIndex((q) => Math.hypot(q[0] - p.x, q[1] - p.y) < 10)
  if (index < 0) {
    const made = insertCurvePoint(list, p.x, p.y)
    index = made.index
    send(made.points, false)
  }
  dragging.value = { index, out: false }
  graph.value.setPointerCapture?.(event.pointerId)
  event.preventDefault()
}

function onMove(event) {
  const d = dragging.value
  if (!d) return
  const p = toGraph(event)
  const list = points.value
  const ends = d.index === 0 || d.index === list.length - 1
  const out = !ends && list.length > 2 && (p.x < -20 || p.x > 275 || p.y < -20 || p.y > 275)
  dragging.value = { ...d, out }
  if (!out) send(moveCurvePoint(list, d.index, p.x, p.y), false)
}

function onUp() {
  const d = dragging.value
  if (!d) return
  dragging.value = null
  send(d.out ? removeCurvePoint(points.value, d.index) : points.value, true)
}

function resetChannel() {
  if (channel.value === 'rgb') send(IDENTITY, true)
  else {
    const next = { ...props.params }
    delete next[channel.value]
    emit('update', next, true)
  }
}
</script>

<template>
  <div class="curves">
    <div class="channels" role="tablist" aria-label="Channel">
      <button v-for="c in CURVE_CHANNELS" :key="c" type="button" role="tab" :aria-selected="channel === c" :class="{ on: channel === c }" @click="channel = c">{{ NAMES[c] }}</button>
      <button type="button" class="reset" title="This channel back to a straight line" @click="resetChannel">Reset</button>
    </div>
    <div class="box">
      <div class="ramp" aria-hidden="true"></div>
      <svg
        ref="graph"
        class="graph"
        viewBox="-4 -4 264 264"
        preserveAspectRatio="none"
        role="img"
        :aria-label="`${NAMES[channel]} curve`"
        @pointerdown="onDown"
        @pointermove="onMove"
        @pointerup="onUp"
        @pointercancel="onUp"
      >
        <path v-if="histPath" class="hist" :d="histPath" />
        <path class="grid" d="M64 0V256M128 0V256M192 0V256M0 64H256M0 128H256M0 192H256" />
        <path class="diag" d="M0 256L256 0" />
        <path class="curve" :d="curvePath" :style="{ stroke: COLOURS[channel] }" />
        <circle
          v-for="(p, i) in points"
          :key="i"
          class="pt"
          :class="{ gone: dragging?.index === i && dragging.out }"
          :cx="p[0]"
          :cy="255 - p[1]"
          r="5"
          :style="{ fill: COLOURS[channel] }"
        />
      </svg>
    </div>
    <p class="dim">Click to add a point, drag it out to take it away.</p>
  </div>
</template>

<style scoped>
.curves {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.channels {
  display: flex;
  gap: 4px;
}

.channels button {
  padding: 3px 10px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: none;
  color: var(--text-dim);
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}

.channels button.on {
  border-color: var(--accent);
  color: var(--text);
  background: color-mix(in srgb, var(--accent) 20%, transparent);
}

.channels .reset {
  margin-left: auto;
}

.box {
  display: flex;
  gap: 6px;
}

.ramp {
  width: 8px;
  border-radius: 3px;
  background: linear-gradient(#fff, #000);
  opacity: 0.6;
}

.graph {
  flex: 1;
  aspect-ratio: 1;
  max-height: 240px;
  border: 1px solid var(--line);
  border-radius: 6px;
  background: rgba(0, 0, 0, 0.25);
  cursor: crosshair;
  touch-action: none;
}

.hist {
  fill: color-mix(in srgb, var(--accent, #a98bff) 22%, transparent);
}

.grid {
  fill: none;
  stroke: rgba(255, 255, 255, 0.08);
  stroke-width: 1;
}

.diag {
  fill: none;
  stroke: rgba(255, 255, 255, 0.15);
  stroke-dasharray: 4 4;
}

.curve {
  fill: none;
  stroke-width: 2;
  filter: drop-shadow(0 0 3px rgba(169, 139, 255, 0.6));
}

.pt {
  stroke: #fff;
  stroke-width: 1.2;
}

.pt.gone {
  opacity: 0.25;
}

.dim {
  margin: 0;
  color: var(--text-dim);
  font-size: 11.5px;
}
</style>
