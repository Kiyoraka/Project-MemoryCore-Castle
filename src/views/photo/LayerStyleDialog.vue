<script setup>
import { computed, onBeforeUnmount, onMounted, reactive, ref } from 'vue'
import GlassSelect from '../../components/GlassSelect.vue'
import { BLEND_MODES_3, STYLE_KEYS, STYLE_NAMES, STYLE_POSITIONS, blendLabel, defaultStyle, normaliseStyle } from './viewHelpers.js'

// PHOTO v3's LAYER STYLE (photo-v3-1): a floating window over the canvas. The effects on the left (a tick
// turns one on, a click shows its settings on the right: blend mode, colour, opacity, the angle dial with
// Use Global Light, distance, spread, size - or what that effect has). Every change shows on the picture at
// once (preview); Cancel puts the layer back as it was, OK keeps it as one history step. Drag it by its title.
const props = defineProps({
  layer: { type: Object, required: true },
  blends: { type: Array, default: () => BLEND_MODES_3 },
  start: { type: String, default: '' }, // the effect to show first
})
const emit = defineEmits(['preview', 'ok', 'cancel'])

const ANGLED = ['dropShadow', 'innerShadow', 'bevel', 'gradientOverlay']
const GLOBAL = ['dropShadow', 'innerShadow', 'bevel']

// every effect present, the layer's own over the defaults
const styles = reactive(Object.fromEntries(STYLE_KEYS.map((k) => [k, props.layer.styles?.[k] ? normaliseStyle(k, props.layer.styles[k], props.blends) : defaultStyle(k)])))
const first = props.start || STYLE_KEYS.find((k) => props.layer.styles?.[k]?.on) || 'dropShadow'
const current = ref(first)
const globalLight = ref(true)
const pos = ref({ x: null, y: null })
const win = ref(null)
let drag = null

const s = computed(() => styles[current.value])

// The styles the layer keeps: the ones he has touched or turned on.
function result() {
  const out = {}
  for (const k of STYLE_KEYS) {
    const had = !!props.layer.styles?.[k]
    if (styles[k].on || had) out[k] = JSON.parse(JSON.stringify(styles[k]))
  }
  return Object.keys(out).length ? out : null
}

function changed() {
  emit('preview', result())
}

function set(key, value) {
  styles[current.value][key] = value
  if (key === 'angle' && globalLight.value && GLOBAL.includes(current.value)) for (const k of GLOBAL) styles[k].angle = value
  // touching a setting turns the effect on, as in Photoshop
  styles[current.value].on = true
  changed()
}

function toggle(k) {
  styles[k].on = !styles[k].on
  current.value = k
  changed()
}

function setStop(i, color) {
  const stops = styles.gradientOverlay.stops.map((x, j) => (j === i ? { ...x, color } : x))
  set('stops', stops)
}

// the angle dial: drag around its centre (0° points right, counter-clockwise like Photoshop)
const dial = ref(null)
let dialing = false
function dialAt(event) {
  const r = dial.value.getBoundingClientRect()
  const a = Math.atan2(r.top + r.height / 2 - event.clientY, event.clientX - (r.left + r.width / 2))
  set('angle', Math.round(((a * 180) / Math.PI + 360) % 360))
}
function dialDown(event) {
  dialing = true
  event.currentTarget.setPointerCapture?.(event.pointerId)
  dialAt(event)
}
function dialMove(event) {
  if (dialing) dialAt(event)
}

const needle = computed(() => {
  const a = ((s.value.angle ?? 120) * Math.PI) / 180
  return { x: 12 + Math.cos(a) * 8, y: 12 - Math.sin(a) * 8 }
})

function grab(event) {
  if (event.target.closest('button')) return
  const r = win.value.getBoundingClientRect()
  const parent = win.value.offsetParent?.getBoundingClientRect() || { left: 0, top: 0 }
  drag = { dx: event.clientX - r.left, dy: event.clientY - r.top, px: parent.left, py: parent.top }
  event.currentTarget.setPointerCapture?.(event.pointerId)
}
function move(event) {
  if (!drag) return
  pos.value = { x: event.clientX - drag.px - drag.dx, y: event.clientY - drag.py - drag.dy }
}

function onKey(event) {
  if (event.key === 'Escape') {
    event.stopPropagation()
    emit('cancel')
  }
}

onMounted(() => {
  window.addEventListener('keydown', onKey, true)
  changed()
})
onBeforeUnmount(() => window.removeEventListener('keydown', onKey, true))

const style = computed(() => (pos.value.x == null ? {} : { left: `${pos.value.x}px`, top: `${pos.value.y}px`, right: 'auto', bottom: 'auto' }))
</script>

<template>
  <form ref="win" class="layer-style" role="dialog" aria-label="Layer style" :style="style" @submit.prevent="emit('ok', result())">
    <header @pointerdown="grab" @pointermove="move" @pointerup="drag = null">
      <h3>LAYER STYLE <span class="dim">· {{ layer.name }}</span></h3>
      <button type="button" class="x" aria-label="Cancel" @click="emit('cancel')">×</button>
    </header>
    <div class="body">
      <ul class="effects">
        <li v-for="k in STYLE_KEYS" :key="k" :class="{ on: current === k }">
          <input type="checkbox" :checked="styles[k].on" :aria-label="`${STYLE_NAMES[k]} on`" @change="toggle(k)" />
          <button type="button" @click="current = k">{{ STYLE_NAMES[k] }}</button>
        </li>
      </ul>
      <div class="settings">
        <h4>{{ STYLE_NAMES[current] }}</h4>
        <label v-if="'blend' in s" class="row">
          <span>Blend mode</span>
          <GlassSelect :model-value="s.blend" :options="blends.map((b) => ({ value: b, label: blendLabel(b) }))" aria-label="Blend mode" @change="(v) => set('blend', v)" />
          <input v-if="'color' in s" type="color" :value="s.color" aria-label="Colour" @input="set('color', $event.target.value)" />
        </label>
        <label v-else-if="'color' in s" class="row">
          <span>Colour</span>
          <input type="color" :value="s.color" aria-label="Colour" @input="set('color', $event.target.value)" />
        </label>
        <template v-if="current === 'bevel'">
          <label class="row">
            <span>Highlight</span>
            <input type="color" :value="s.highlight" aria-label="Highlight colour" @input="set('highlight', $event.target.value)" />
            <span>Shadow</span>
            <input type="color" :value="s.shadow" aria-label="Shadow colour" @input="set('shadow', $event.target.value)" />
          </label>
          <label class="row">
            <span>Direction</span>
            <GlassSelect :model-value="s.direction" :options="[{ value: 'up', label: 'Up' }, { value: 'down', label: 'Down' }]" aria-label="Direction" @change="(v) => set('direction', v)" />
          </label>
        </template>
        <label v-if="current === 'stroke'" class="row">
          <span>Position</span>
          <GlassSelect :model-value="s.position" :options="STYLE_POSITIONS" aria-label="Position" @change="(v) => set('position', v)" />
        </label>
        <label class="row">
          <span>Opacity</span>
          <input type="range" min="0" max="100" :value="Math.round(s.opacity * 100)" @input="set('opacity', $event.target.value / 100)" />
          <span class="num">{{ Math.round(s.opacity * 100) }}%</span>
        </label>
        <div v-if="ANGLED.includes(current)" class="row">
          <span>Angle</span>
          <svg ref="dial" class="dial" viewBox="0 0 24 24" role="slider" :aria-valuenow="s.angle" aria-valuemin="0" aria-valuemax="359" aria-label="Angle" tabindex="0" @pointerdown="dialDown" @pointermove="dialMove" @pointerup="dialing = false" @keydown.left.prevent="set('angle', (s.angle + 1) % 360)" @keydown.right.prevent="set('angle', (s.angle + 359) % 360)">
            <circle cx="12" cy="12" r="10" />
            <line x1="12" y1="12" :x2="needle.x" :y2="needle.y" />
          </svg>
          <input class="deg" type="number" min="0" max="359" :value="s.angle" aria-label="Angle in degrees" @change="set('angle', ((Number($event.target.value) % 360) + 360) % 360)" />
          <span>°</span>
          <label v-if="GLOBAL.includes(current)" class="check"><input v-model="globalLight" type="checkbox" /> Use Global Light</label>
        </div>
        <label v-if="'altitude' in s" class="row">
          <span>Altitude</span>
          <input type="range" min="0" max="90" :value="s.altitude" @input="set('altitude', Number($event.target.value))" />
          <span class="num">{{ s.altitude }}°</span>
        </label>
        <label v-if="'distance' in s" class="row">
          <span>Distance</span>
          <input type="range" min="0" max="250" :value="s.distance" @input="set('distance', Number($event.target.value))" />
          <span class="num">{{ s.distance }} px</span>
        </label>
        <label v-if="'spread' in s" class="row">
          <span>Spread</span>
          <input type="range" min="0" max="100" :value="s.spread" @input="set('spread', Number($event.target.value))" />
          <span class="num">{{ s.spread }}%</span>
        </label>
        <label v-if="'depth' in s" class="row">
          <span>Depth</span>
          <input type="range" min="1" max="1000" :value="s.depth" @input="set('depth', Number($event.target.value))" />
          <span class="num">{{ s.depth }}%</span>
        </label>
        <label v-if="'size' in s" class="row">
          <span>Size</span>
          <input type="range" min="0" max="250" :value="s.size" @input="set('size', Number($event.target.value))" />
          <span class="num">{{ s.size }} px</span>
        </label>
        <label v-if="'width' in s" class="row">
          <span>Size</span>
          <input type="range" min="1" max="100" :value="s.width" @input="set('width', Number($event.target.value))" />
          <span class="num">{{ s.width }} px</span>
        </label>
        <div v-if="current === 'gradientOverlay'" class="row">
          <span>Gradient</span>
          <input type="color" :value="s.stops[0].color" aria-label="Gradient start" @input="setStop(0, $event.target.value)" />
          <span class="ramp" :style="{ background: `linear-gradient(90deg, ${s.stops.map((x) => `${x.color} ${x.at * 100}%`).join(', ')})` }"></span>
          <input type="color" :value="s.stops.at(-1).color" aria-label="Gradient end" @input="setStop(s.stops.length - 1, $event.target.value)" />
        </div>
      </div>
    </div>
    <footer>
      <button type="button" @click="emit('cancel')">Cancel</button>
      <button type="submit" class="primary">OK</button>
    </footer>
  </form>
</template>

<style scoped>
.layer-style {
  position: absolute;
  right: 380px;
  bottom: 120px;
  z-index: 48;
  display: flex;
  flex-direction: column;
  width: min(470px, 92vw);
  border: 1px solid var(--line);
  border-radius: 16px;
  background: var(--panel-background);
  backdrop-filter: blur(var(--glass-blur));
  box-shadow: 0 18px 46px rgba(0, 0, 0, 0.55);
  color: var(--text);
}

header {
  display: flex;
  align-items: center;
  padding: 10px 14px;
  border-bottom: 1px solid var(--line);
  cursor: move;
  touch-action: none;
}

h3 {
  flex: 1;
  margin: 0;
  color: var(--text-dim);
  font-size: 13px;
  font-weight: 500;
  letter-spacing: 0.16em;
}

h4 {
  margin: 0 0 2px;
  color: var(--accent);
  font-size: 12.5px;
  font-weight: 600;
}

.body {
  display: grid;
  grid-template-columns: 150px 1fr;
}

.effects {
  margin: 0;
  padding: 8px 0;
  border-right: 1px solid var(--line);
  list-style: none;
}

.effects li {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 2px 10px;
}

.effects li.on {
  background: color-mix(in srgb, var(--accent) 18%, transparent);
}

.effects input {
  accent-color: var(--accent);
}

.effects button {
  flex: 1;
  padding: 5px 0;
  border: 0;
  background: none;
  color: var(--text);
  font-size: 13px;
  text-align: left;
}

.settings {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px 14px;
}

.row {
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--text-dim);
  font-size: 12px;
}

.row > span:first-child {
  width: 70px;
  flex: none;
}

.row input[type='range'] {
  flex: 1;
  min-width: 40px;
  accent-color: var(--accent);
}

.row select {
  flex: 1;
  min-width: 0;
  padding: 3px 6px;
  border: 1px solid var(--line);
  border-radius: 6px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 12px;
}

input[type='color'] {
  width: 30px;
  height: 22px;
  padding: 0;
  border: 1px solid var(--line);
  border-radius: 4px;
  background: none;
}

.num {
  width: 46px;
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.dial {
  width: 34px;
  height: 34px;
  flex: none;
  cursor: grab;
  touch-action: none;
}

.dial circle {
  fill: rgba(0, 0, 0, 0.25);
  stroke: var(--text-dim);
  stroke-width: 1.2;
}

.dial line {
  stroke: var(--text);
  stroke-width: 1.6;
  stroke-linecap: round;
}

.deg {
  width: 52px;
  padding: 3px 6px;
  border: 1px solid var(--line);
  border-radius: 6px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 12px;
}

.check {
  display: flex;
  align-items: center;
  gap: 4px;
  margin-left: auto;
  font-size: 11.5px;
}

.check input {
  accent-color: var(--accent);
}

.ramp {
  flex: 1;
  height: 14px;
  border: 1px solid var(--line);
  border-radius: 4px;
}

footer {
  display: flex;
  justify-content: flex-end;
  gap: 10px;
  padding: 10px 14px 14px;
}

footer button {
  min-width: 90px;
  padding: 6px 16px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: none;
  color: var(--text);
  font-size: 13px;
}

footer button.primary {
  border-color: var(--accent);
  background: var(--accent);
  color: #160f33;
}

.x {
  padding: 0 4px;
  border: 0;
  background: none;
  color: var(--text-dim);
  font-size: 18px;
}

.dim {
  color: var(--text-dim);
  font-size: 11.5px;
  letter-spacing: 0;
}

button {
  font: inherit;
  cursor: pointer;
}

button:focus-visible,
select:focus-visible,
input:focus-visible,
.dial:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
</style>
