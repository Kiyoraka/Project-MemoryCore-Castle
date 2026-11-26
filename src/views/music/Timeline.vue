<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { clock } from './musicActions.js'
import { sectionColor } from './colors.js'

// The one large timeline: the chosen take's waveform cut into coloured sections, the playhead, a
// click-drag selection (section.select) with its button above it, a click to move the playhead,
// Ctrl+wheel to zoom.
const props = defineProps({
  peaks: { type: Array, default: () => [] },
  duration: { type: Number, default: 0 },
  sections: { type: Array, default: () => [] },
  selection: { type: Object, default: null },
  at: { type: Number, default: 0 },
  // the button above the selection (FIX): its words, or '' for none
  action: { type: String, default: '' },
  // her hand just lit the selection
  hers: { type: Boolean, default: false },
  note: { type: String, default: '' },
})
const emit = defineEmits(['select', 'seek', 'act'])

const scroller = ref(null)
const track = ref(null)
const canvas = ref(null)
const zoom = ref(1)
const dragging = ref(null) // { from, to } while the pointer is down
let observer
const HEIGHT = 120

const pct = (s) => (props.duration > 0 ? `${Math.min(100, Math.max(0, (s / props.duration) * 100))}%` : '0%')
const span = (a, b) => (props.duration > 0 ? `${Math.max(0, ((b - a) / props.duration) * 100)}%` : '0%')
const shown = computed(() => dragging.value ?? props.selection)

function timeAt(event) {
  const box = track.value.getBoundingClientRect()
  return Math.max(0, Math.min(1, (event.clientX - box.left) / box.width)) * props.duration
}

function down(event) {
  if (!props.duration || event.button !== 0) return
  const t = timeAt(event)
  dragging.value = { from: t, to: t, start: t }
  track.value.setPointerCapture(event.pointerId)
}

function move(event) {
  if (!dragging.value) return
  const t = timeAt(event)
  const s = dragging.value.start
  dragging.value = { start: s, from: Math.min(s, t), to: Math.max(s, t) }
}

function up(event) {
  const d = dragging.value
  if (!d) return
  dragging.value = null
  track.value.releasePointerCapture?.(event.pointerId)
  // a short press is a click: the playhead moves there
  if (d.to - d.from < Math.max(0.3, props.duration / 400)) emit('seek', d.start)
  else emit('select', { from: d.from, to: d.to })
}

function pickSection(s) {
  emit('select', { from: s.from, to: s.to })
}

function wheel(event) {
  if (!event.ctrlKey) return
  event.preventDefault()
  const el = scroller.value
  const box = el.getBoundingClientRect()
  const pointer = event.clientX - box.left
  const before = (el.scrollLeft + pointer) / (box.width * zoom.value)
  zoom.value = Math.min(16, Math.max(1, zoom.value * (event.deltaY < 0 ? 1.25 : 0.8)))
  nextTick(() => {
    el.scrollLeft = before * box.width * zoom.value - pointer
  })
}

function draw() {
  const c = canvas.value
  if (!c || !track.value) return
  const width = track.value.clientWidth
  const dpr = window.devicePixelRatio || 1
  c.width = Math.max(1, Math.round(width * dpr))
  c.height = Math.round(HEIGHT * dpr)
  const g = c.getContext('2d')
  g.setTransform(dpr, 0, 0, dpr, 0, 0)
  g.clearRect(0, 0, width, HEIGHT)
  const n = props.peaks.length
  if (!n) {
    g.fillStyle = 'color-mix(in srgb, var(--accent) 25%, transparent)'
    g.fillRect(0, HEIGHT / 2 - 1, width, 2)
    return
  }
  const bar = width / n
  const mid = HEIGHT / 2
  for (let i = 0; i < n; i += 1) {
    const t = ((i + 0.5) / n) * props.duration
    const at = props.sections.findIndex((s) => t >= s.from && t < s.to)
    g.fillStyle = sectionColor(at < 0 ? 0 : at)
    const h = Math.max(1.5, props.peaks[i] * (HEIGHT - 12))
    g.fillRect(i * bar, mid - h / 2, Math.max(1, bar - 0.6), h)
  }
}

watch(() => [props.peaks, props.sections, props.duration, zoom.value], () => nextTick(draw), { deep: true })

// the playhead stays in view while it plays through a zoomed song
watch(
  () => props.at,
  (at) => {
    const el = scroller.value
    if (!el || zoom.value === 1 || !props.duration || dragging.value) return
    const x = (at / props.duration) * el.scrollWidth
    if (x < el.scrollLeft || x > el.scrollLeft + el.clientWidth - 20) el.scrollLeft = x - 40
  },
)

onMounted(() => {
  observer = new ResizeObserver(() => draw())
  observer.observe(track.value)
  draw()
})
onBeforeUnmount(() => observer?.disconnect())
</script>

<template>
  <div class="timeline">
    <header>
      <span class="title">TIMELINE</span>
      <span class="times">{{ clock(at) }} / {{ clock(duration) }}</span>
      <span v-if="shown" class="sel">selected {{ clock(shown.from) }}-{{ clock(shown.to) }}</span>
      <span class="zoom" title="Ctrl + wheel zooms">{{ zoom > 1 ? `${zoom.toFixed(1)}x` : 'Ctrl+wheel to zoom' }}</span>
    </header>
    <p v-if="note" class="note">{{ note }}</p>
    <div ref="scroller" class="scroller" @wheel="wheel">
      <div class="inner" :style="{ width: `${zoom * 100}%` }">
        <div class="labels">
          <button
            v-for="(s, n) in sections"
            :key="`${s.name}-${s.from}`"
            class="label"
            :style="{ left: pct(s.from), width: span(s.from, s.to), '--c': sectionColor(n) }"
            :title="`${s.name} ${clock(s.from)}-${clock(s.to)} - click to select`"
            @click="pickSection(s)"
          >{{ s.name }}</button>
        </div>
        <div
          ref="track"
          class="track"
          :class="{ empty: !duration }"
          @pointerdown="down"
          @pointermove="move"
          @pointerup="up"
          @pointercancel="dragging = null"
        >
          <span
            v-for="(s, n) in sections"
            :key="`bg-${s.name}-${s.from}`"
            class="band"
            :style="{ left: pct(s.from), width: span(s.from, s.to), '--c': sectionColor(n) }"
          ></span>
          <canvas ref="canvas" class="wave" :style="{ height: `${HEIGHT}px` }"></canvas>
          <div v-if="shown" class="selection" :class="{ hers }" :style="{ left: pct(shown.from), width: span(shown.from, shown.to) }">
            <button v-if="action && !dragging" class="act" @pointerdown.stop @click.stop="emit('act')">&#8767; {{ action }}</button>
          </div>
          <span v-if="duration" class="playhead" :style="{ left: pct(at) }"></span>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.timeline {
  display: flex;
  flex-direction: column;
  gap: 6px;
  min-width: 0;
}

header {
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: 11.5px;
  color: var(--text-dim);
}

.title {
  letter-spacing: 0.16em;
  color: var(--text);
  font-weight: 600;
}

.sel {
  color: #ffd59a;
}

.zoom {
  margin-left: auto;
}

.note {
  margin: 0;
  font-size: 12px;
  color: var(--text-dim);
}

.scroller {
  overflow-x: auto;
  overflow-y: hidden;
  padding-top: 34px;
  margin-top: -34px;
}

.inner {
  position: relative;
  min-width: 100%;
}

.labels {
  position: relative;
  height: 20px;
}

.label {
  position: absolute;
  top: 0;
  overflow: hidden;
  padding: 1px 6px;
  border: 0;
  border-bottom: 2px solid var(--c);
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 11px;
  text-align: left;
  text-overflow: ellipsis;
  white-space: nowrap;
  cursor: pointer;
  opacity: 0.85;
}

.label:hover {
  opacity: 1;
  background: rgba(255, 255, 255, 0.05);
}

.track {
  position: relative;
  height: 120px;
  border: 1px solid var(--line);
  border-radius: 12px;
  overflow: visible;
  background: rgba(0, 0, 0, 0.28);
  cursor: crosshair;
  touch-action: none;
}

.track.empty {
  cursor: default;
}

.band {
  position: absolute;
  top: 0;
  bottom: 0;
  background: color-mix(in srgb, var(--c) 16%, transparent);
  border-right: 1px solid rgba(255, 255, 255, 0.06);
  pointer-events: none;
}

.wave {
  position: absolute;
  inset: 0;
  width: 100%;
  pointer-events: none;
}

.selection {
  position: absolute;
  top: -2px;
  bottom: -2px;
  border: 2px solid #ffc977;
  border-radius: 10px;
  background: rgba(255, 190, 100, 0.16);
  box-shadow: 0 0 18px rgba(255, 190, 100, 0.55);
  pointer-events: none;
}

.selection.hers {
  animation: hers 1.6s ease-out;
}

@keyframes hers {
  0% { box-shadow: 0 0 0 0 rgba(255, 210, 122, 1), 0 0 30px rgba(255, 210, 122, 0.9); }
  100% { box-shadow: 0 0 18px rgba(255, 190, 100, 0.55); }
}

.act {
  position: absolute;
  bottom: calc(100% + 8px);
  left: 50%;
  transform: translateX(-50%);
  padding: 4px 12px;
  border: 1px solid var(--accent);
  border-radius: 999px;
  background: linear-gradient(135deg, color-mix(in srgb, var(--accent) 70%, #000), var(--accent));
  color: #fff;
  font: inherit;
  font-size: 12px;
  white-space: nowrap;
  cursor: pointer;
  pointer-events: auto;
  box-shadow: 0 0 14px color-mix(in srgb, var(--accent) 60%, transparent);
}

.act::after {
  content: '';
  position: absolute;
  top: 100%;
  left: 50%;
  border: 6px solid transparent;
  border-top-color: var(--accent);
  transform: translateX(-50%);
}

.playhead {
  position: absolute;
  top: -4px;
  bottom: -4px;
  width: 2px;
  margin-left: -1px;
  background: #fff;
  box-shadow: 0 0 8px rgba(255, 255, 255, 0.9);
  pointer-events: none;
}
</style>
