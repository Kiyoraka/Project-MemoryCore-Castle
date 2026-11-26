<script setup>
import { computed, ref } from 'vue'
import { gammaToPos, posToGamma } from './viewHelpers.js'

// PHOTO's levels: the histogram with three triangles under it - black, grey (gamma) and white.
const props = defineProps({
  params: { type: Object, default: () => ({ black: 0, gamma: 1, white: 255 }) },
  histogram: { type: Object, default: null },
})
const emit = defineEmits(['update'])

const bar = ref(null)
const dragging = ref('')

const black = computed(() => props.params?.black ?? 0)
const white = computed(() => props.params?.white ?? 255)
const gamma = computed(() => props.params?.gamma ?? 1)
const grey = computed(() => gammaToPos(black.value, white.value, gamma.value))

const histPath = computed(() => {
  const bins = props.histogram?.luma
  if (!bins?.length) return ''
  let max = 0
  for (let i = 1; i < 255; i++) if (bins[i] > max) max = bins[i]
  if (!max) return ''
  let d = 'M0 100'
  for (let i = 0; i < 256; i++) d += `L${i} ${100 - Math.min(1, bins[i] / max) * 96}`
  return `${d}L255 100Z`
})

function send(fields, final) {
  emit('update', { black: black.value, white: white.value, gamma: gamma.value, ...fields }, final)
}

function valueAt(event) {
  const r = bar.value.getBoundingClientRect()
  return Math.max(0, Math.min(255, ((event.clientX - r.left) / r.width) * 255))
}

function grab(which, event) {
  dragging.value = which
  bar.value.setPointerCapture?.(event.pointerId)
  event.preventDefault()
}

function onMove(event) {
  const which = dragging.value
  if (!which) return
  const v = Math.round(valueAt(event))
  if (which === 'black') send({ black: Math.min(v, white.value - 2) }, false)
  else if (which === 'white') send({ white: Math.max(v, black.value + 2) }, false)
  else send({ gamma: Math.round(posToGamma(black.value, white.value, v) * 100) / 100 }, false)
}

function onUp() {
  if (!dragging.value) return
  dragging.value = ''
  send({}, true)
}

function typed(key, event) {
  const v = Number(event.target.value)
  if (!Number.isFinite(v)) return
  if (key === 'black') send({ black: Math.max(0, Math.min(white.value - 2, Math.round(v))) }, true)
  else if (key === 'white') send({ white: Math.min(255, Math.max(black.value + 2, Math.round(v))) }, true)
  else send({ gamma: Math.max(0.1, Math.min(9.99, v)) }, true)
}

const at = (v) => `${(v / 255) * 100}%`
</script>

<template>
  <div class="levels">
    <svg class="hist" viewBox="0 0 255 100" preserveAspectRatio="none" aria-hidden="true">
      <path v-if="histPath" :d="histPath" />
      <text v-else x="127" y="55" text-anchor="middle">no histogram yet</text>
    </svg>
    <div ref="bar" class="bar" @pointermove="onMove" @pointerup="onUp" @pointercancel="onUp">
      <div class="ramp"></div>
      <button type="button" class="tri black" :style="{ left: at(black) }" aria-label="Black point" @pointerdown="grab('black', $event)"></button>
      <button type="button" class="tri grey" :style="{ left: at(grey) }" aria-label="Grey point" @pointerdown="grab('grey', $event)"></button>
      <button type="button" class="tri white" :style="{ left: at(white) }" aria-label="White point" @pointerdown="grab('white', $event)"></button>
    </div>
    <div class="nums">
      <label><span>Black</span><input type="number" min="0" max="253" :value="black" @change="typed('black', $event)" /></label>
      <label><span>Gamma</span><input type="number" min="0.1" max="9.99" step="0.01" :value="gamma.toFixed(2)" @change="typed('gamma', $event)" /></label>
      <label><span>White</span><input type="number" min="2" max="255" :value="white" @change="typed('white', $event)" /></label>
    </div>
  </div>
</template>

<style scoped>
.levels {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.hist {
  width: 100%;
  height: 110px;
  border: 1px solid var(--line);
  border-radius: 6px;
  background: rgba(0, 0, 0, 0.25);
}

.hist path {
  fill: color-mix(in srgb, var(--accent, #a98bff) 45%, transparent);
}

.hist text {
  fill: var(--text-dim);
  font-size: 10px;
}

.bar {
  position: relative;
  height: 22px;
  touch-action: none;
}

.ramp {
  position: absolute;
  top: 0;
  left: 0;
  right: 0;
  height: 6px;
  border-radius: 3px;
  background: linear-gradient(90deg, #000, #fff);
}

.tri {
  position: absolute;
  top: 6px;
  width: 14px;
  height: 14px;
  padding: 0;
  border: 0;
  background: none;
  transform: translateX(-50%);
  cursor: ew-resize;
}

.tri::before {
  content: '';
  position: absolute;
  inset: 0;
  clip-path: polygon(50% 0, 100% 100%, 0 100%);
  border: 0;
}

.tri.black::before {
  background: #111;
  outline: 1px solid #fff;
}

.tri.grey::before {
  background: #888;
}

.tri.white::before {
  background: #fff;
}

.tri:focus-visible {
  outline: 2px solid var(--accent);
}

.nums {
  display: flex;
  gap: 8px;
}

.nums label {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 2px;
  color: var(--text-dim);
  font-size: 11px;
}

.nums input {
  width: 100%;
  padding: 3px 6px;
  border: 1px solid var(--line);
  border-radius: 6px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 12px;
}
</style>
