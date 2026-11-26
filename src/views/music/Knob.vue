<script setup>
import { computed, ref } from 'vue'

// One dial of the bar: drag up or down, the wheel, or the arrow keys. It turns smoothly when her
// hand moves it (the rotation is animated; his own drag follows the pointer at once).
const props = defineProps({
  label: { type: String, required: true },
  value: { type: Number, required: true },
  min: { type: Number, default: 0 },
  max: { type: Number, default: 1 },
  step: { type: Number, default: 0.01 },
  shown: { type: String, default: '' },
  hers: { type: Boolean, default: false },
})
const emit = defineEmits(['change'])

const turning = ref(false)
let start = null

const angle = computed(() => -135 + ((props.value - props.min) / (props.max - props.min || 1)) * 270)

function set(v) {
  const snapped = Math.round(Math.min(props.max, Math.max(props.min, v)) / props.step) * props.step
  const clean = Math.round(snapped * 1000) / 1000
  if (clean !== props.value) emit('change', clean)
}

function down(event) {
  turning.value = true
  start = { y: event.clientY, value: props.value }
  event.currentTarget.setPointerCapture(event.pointerId)
}

function move(event) {
  if (!start) return
  set(start.value + ((start.y - event.clientY) / 160) * (props.max - props.min))
}

function up() {
  start = null
  turning.value = false
}

function wheel(event) {
  event.preventDefault()
  set(props.value + (event.deltaY < 0 ? props.step : -props.step))
}

function key(event) {
  const by = { ArrowUp: 1, ArrowRight: 1, ArrowDown: -1, ArrowLeft: -1, PageUp: 10, PageDown: -10 }[event.key]
  if (!by) return
  event.preventDefault()
  set(props.value + by * props.step)
}
</script>

<template>
  <div class="knob-box" :class="{ hers }">
    <div
      class="knob"
      role="slider"
      tabindex="0"
      :aria-label="label"
      :aria-valuemin="min"
      :aria-valuemax="max"
      :aria-valuenow="value"
      :aria-valuetext="shown || String(value)"
      @pointerdown="down"
      @pointermove="move"
      @pointerup="up"
      @pointercancel="up"
      @wheel="wheel"
      @keydown="key"
    >
      <span class="face" :class="{ turning }" :style="{ transform: `rotate(${angle}deg)` }"><i></i></span>
    </div>
    <span class="value">{{ shown || value }}</span>
    <span class="label">{{ label }}</span>
  </div>
</template>

<style scoped>
.knob-box {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 3px;
  min-width: 64px;
}

.knob {
  display: grid;
  place-items: center;
  width: 54px;
  height: 54px;
  border-radius: 50%;
  background: conic-gradient(from 225deg, var(--accent), color-mix(in srgb, var(--accent) 50%, #4f7dff) 75%, transparent 75%);
  cursor: ns-resize;
  touch-action: none;
  outline-offset: 3px;
}

.face {
  position: relative;
  width: 44px;
  height: 44px;
  border-radius: 50%;
  background: radial-gradient(circle at 35% 30%, color-mix(in srgb, var(--accent) 35%, #111), #080812 70%);
  box-shadow: inset 0 2px 6px rgba(255, 255, 255, 0.12), 0 2px 8px rgba(0, 0, 0, 0.5);
  transition: transform 0.6s cubic-bezier(0.2, 0.8, 0.2, 1);
}

.face.turning {
  transition: none;
}

.face i {
  position: absolute;
  top: 5px;
  left: 50%;
  width: 3px;
  height: 11px;
  margin-left: -1.5px;
  border-radius: 2px;
  background: #fff;
  box-shadow: 0 0 6px rgba(255, 255, 255, 0.8);
}

.value {
  font-size: 12px;
  color: var(--text);
}

.label {
  font-size: 10.5px;
  letter-spacing: 0.06em;
  color: var(--text-dim);
}

.hers .knob {
  animation: hers 1.6s ease-out;
}

@keyframes hers {
  0% { box-shadow: 0 0 0 0 rgba(255, 210, 122, 1), 0 0 22px rgba(255, 210, 122, 0.9); }
  100% { box-shadow: 0 0 0 12px rgba(255, 210, 122, 0); }
}
</style>
