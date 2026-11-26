<script setup>
import { computed } from 'vue'
import GlassSelect from '../../components/GlassSelect.vue'
import { ADJUSTMENT_NAMES, ADJUSTMENT_SPECS, BRUSH_PRESETS, cleanSpecParams, fromTransformNumbers, getPath, setPath, specFields, transformNumbers } from './viewHelpers.js'
import AdjustPanel from './AdjustPanel.vue'
import CurvesEditor from './CurvesEditor.vue'
import LevelsEditor from './LevelsEditor.vue'
import HslEditor from './HslEditor.vue'

// PHOTO's PROPERTIES: follows what he is doing - the numbers of a free transform, the chosen tool's
// settings (brush with its presets and pen pressure, the retouch brushes, wand, gradient stops, shape fill
// and stroke), a text layer's type (paragraph box: align, line height, tracking), then the chosen adjustment
// layer's editor (v3: a plain slider or colour for every setting of the new types), or the document's own
// eight sliders when no adjustment layer is chosen.
const props = defineProps({
  hasDoc: { type: Boolean, default: false },
  layer: { type: Object, default: null },
  docAdjust: { type: Object, default: () => ({}) },
  tool: { type: String, default: 'move' },
  brush: { type: Object, default: () => ({ size: 24, hardness: 0.7, opacity: 1 }) },
  tolerance: { type: Number, default: 32 },
  gradient: { type: Object, default: () => ({ kind: 'linear', stops: [] }) },
  shape: { type: Object, default: () => ({}) },
  transform: { type: Object, default: null },
  histogram: { type: Object, default: null },
  editMask: { type: Boolean, default: false },
  primary: { type: String, default: '#ffffff' },
  secondary: { type: String, default: '#000000' },
  docked: { type: Boolean, default: false }, // in the dock: the dock's tab is its title
  quickMask: { type: Boolean, default: false },
})
const emit = defineEmits(['adjustment', 'adjust', 'reset-adjust', 'brush', 'tolerance', 'gradient', 'shape', 'transform', 'text', 'close'])

const BRUSHY = ['brush', 'eraser', 'clone', 'heal', 'dodge', 'burn', 'sponge', 'smudge', 'blurBrush', 'sharpenBrush', 'spotHeal', 'quickSelect']
const PAINTS = ['brush', 'eraser']
const RETOUCH = ['dodge', 'burn', 'sponge', 'smudge', 'blurBrush', 'sharpenBrush']
const TOOL_NAMES = {
  brush: 'Brush',
  eraser: 'Eraser',
  clone: 'Clone stamp',
  heal: 'Healing brush',
  dodge: 'Dodge',
  burn: 'Burn',
  sponge: 'Sponge',
  smudge: 'Smudge',
  blurBrush: 'Blur brush',
  sharpenBrush: 'Sharpen brush',
  spotHeal: 'Spot healing brush',
  quickSelect: 'Quick select',
}
const FONTS = ['Inter, "Segoe UI", sans-serif', 'Georgia, "Times New Roman", serif', '"Trebuchet MS", sans-serif', '"Courier New", monospace', 'Impact, sans-serif', '"Brush Script MT", cursive']
const FONT_NAMES = ['Sans', 'Serif', 'Trebuchet', 'Mono', 'Impact', 'Script']
const HINTS = {
  move: 'Drag to move the chosen layer (its mask comes along). Ctrl+T: free transform.',
  rect: 'Drag a rectangle. A click clears the selection.',
  lasso: 'Draw around what you want.',
  crop: 'Drag the area to keep, then Enter. Esc cancels.',
  text: 'Click to add text, or click a text to edit it. Enter finishes.',
  eyedropper: 'Click to pick a colour from the photo.',
  hand: 'Drag to move around the picture.',
  zoom: 'Click to zoom in, Alt+click to zoom out.',
  pen: 'Click points along the path. Enter makes a selection, Shift+Enter a shape (fill and stroke below).',
  heal: 'Alt+click a clean spot, then paint over the blemish.',
  clone: 'Alt+click sets the source.',
  dodge: 'Paint to lighten.',
  burn: 'Paint to darken.',
  sponge: 'Paint to saturate or desaturate.',
  smudge: 'Drag to push the colours along.',
  blurBrush: 'Paint to soften.',
  sharpenBrush: 'Paint to sharpen.',
  spotHeal: 'Paint over a spot - the clean part is found by itself.',
  quickSelect: 'Paint over what you want: the selection grows to its edges. Alt takes away.',
  objectSelect: 'Drag a box around the object.',
  patch: 'Draw around the spot, then drag it onto a clean part.',
}
const SHAPE_KINDS = [
  ['rect', 'Rectangle'],
  ['ellipse', 'Ellipse'],
  ['line', 'Line'],
  ['polygon', 'Polygon'],
]

const adjustment = computed(() => (props.layer?.kind === 'adjustment' ? props.layer.adjustment : null))
const specType = computed(() => (adjustment.value && ADJUSTMENT_SPECS[adjustment.value.type] ? adjustment.value.type : ''))
const fields = computed(() => (specType.value ? specFields(specType.value, adjustment.value.params) : []))
const textLayer = computed(() => (props.layer?.kind === 'text' ? props.layer : null))
const box = computed(() => textLayer.value?.textBox || null)

function specValue(f) {
  return getPath(adjustment.value?.params, f.key) ?? (typeof f.value === 'function' ? f.value() : f.value)
}

// one setting changed: the params cleaned whole (the chosen tone / range / output stays with them)
function setSpec(f, raw, final) {
  const v = f.type === 'check' ? !!raw : f.type === 'color' || f.type === 'select' ? raw : Number(raw)
  emit('adjustment', cleanSpecParams(specType.value, setPath(adjustment.value.params, f.key, v)), final)
}

function setText(fields, final = true) {
  emit('text', { text: { ...textLayer.value.text, ...fields } }, final)
}

function setBox(fields, final = true) {
  emit('text', { textBox: { ...(box.value || { w: 300, h: 0, align: 'left', lineHeight: 1.2, tracking: 0 }), ...fields } }, final)
}

function pickPreset(p) {
  emit('brush', { preset: p.id, hardness: p.hardness, flow: p.flow, build: p.build, texture: p.texture })
}
const title = computed(() => {
  if (props.transform) return 'Free transform'
  if (adjustment.value) return ADJUSTMENT_NAMES[adjustment.value.type] || 'Adjustment'
  return 'Adjust'
})
const numbers = computed(() => (props.transform ? transformNumbers(props.transform) : null))
const showsShape = computed(() => props.tool === 'shape' || props.tool === 'pen')
const stopsCss = computed(() => {
  const stops = [...(props.gradient?.stops || [])].sort((a, b) => a.at - b.at)
  return stops.length ? `linear-gradient(90deg, ${stops.map((s) => `${s.color} ${Math.round(s.at * 100)}%`).join(', ')})` : 'none'
})

function setNumber(key, event) {
  const v = Number(event.target.value)
  if (!Number.isFinite(v) || !props.transform) return
  emit('transform', fromTransformNumbers(props.transform, { ...numbers.value, [key]: v }))
}

function setStop(i, fields) {
  const stops = props.gradient.stops.map((s, j) => (j === i ? { ...s, ...fields } : s))
  emit('gradient', { ...props.gradient, stops })
}

function addStop() {
  const stops = [...props.gradient.stops, { at: 0.5, color: props.primary }]
  emit('gradient', { ...props.gradient, stops })
}

function removeStop(i) {
  if (props.gradient.stops.length <= 2) return
  emit('gradient', { ...props.gradient, stops: props.gradient.stops.filter((_, j) => j !== i) })
}
</script>

<template>
  <section class="props" aria-label="Properties">
    <header v-if="!docked">
      <h2>PROPERTIES</h2>
      <button type="button" class="x" aria-label="Close properties" title="Close" @click="emit('close')">×</button>
    </header>
    <p v-if="!hasDoc" class="dim">Open a photo to see its properties.</p>
    <div v-else class="body">
      <!-- free transform -->
      <div v-if="numbers" class="block">
        <h3>Free transform</h3>
        <div class="grid">
          <label><span>X</span><input type="number" :value="numbers.x" @change="setNumber('x', $event)" /></label>
          <label><span>Y</span><input type="number" :value="numbers.y" @change="setNumber('y', $event)" /></label>
          <label><span>W %</span><input type="number" step="0.1" :value="numbers.w" @change="setNumber('w', $event)" /></label>
          <label><span>H %</span><input type="number" step="0.1" :value="numbers.h" @change="setNumber('h', $event)" /></label>
          <label><span>Angle</span><input type="number" step="0.1" :value="numbers.angle" @change="setNumber('angle', $event)" /></label>
        </div>
        <p class="dim">Drag a corner to scale (Shift keeps the shape), inside to move, outside to turn. Enter applies, Esc cancels.</p>
      </div>

      <template v-else>
        <!-- the tool -->
        <div v-if="BRUSHY.includes(tool)" class="block">
          <h3>{{ quickMask && PAINTS.includes(tool) ? 'Brush on the quick mask' : editMask && layer?.mask ? 'Brush on the mask' : TOOL_NAMES[tool] }}</h3>
          <div v-if="PAINTS.includes(tool)" class="presets" role="radiogroup" aria-label="Brush preset">
            <button
              v-for="p in BRUSH_PRESETS"
              :key="p.id"
              type="button"
              role="radio"
              class="preset"
              :class="{ on: (brush.preset || 'soft') === p.id }"
              :aria-checked="(brush.preset || 'soft') === p.id"
              :title="p.label"
              @click="pickPreset(p)"
            >
              <span class="dot" :class="p.id"></span>
              <span>{{ p.label }}</span>
            </button>
          </div>
          <label class="slide">
            <span>Size</span>
            <input type="range" min="1" max="500" :value="brush.size" aria-label="Brush size" @input="emit('brush', { size: Number($event.target.value) })" />
            <span class="num">{{ brush.size }}px</span>
          </label>
          <label v-if="tool !== 'quickSelect'" class="slide">
            <span>Hardness</span>
            <input type="range" min="0" max="100" :value="Math.round(brush.hardness * 100)" aria-label="Brush hardness" @input="emit('brush', { hardness: $event.target.value / 100 })" />
            <span class="num">{{ Math.round(brush.hardness * 100) }}%</span>
          </label>
          <label v-if="PAINTS.includes(tool)" class="slide">
            <span>Opacity</span>
            <input type="range" min="1" max="100" :value="Math.round(brush.opacity * 100)" aria-label="Brush opacity" @input="emit('brush', { opacity: $event.target.value / 100 })" />
            <span class="num">{{ Math.round(brush.opacity * 100) }}%</span>
          </label>
          <label v-if="PAINTS.includes(tool) && brush.build" class="slide">
            <span>Flow</span>
            <input type="range" min="1" max="100" :value="Math.round((brush.flow ?? 1) * 100)" aria-label="Brush flow" @input="emit('brush', { flow: $event.target.value / 100 })" />
            <span class="num">{{ Math.round((brush.flow ?? 1) * 100) }}%</span>
          </label>
          <label v-if="RETOUCH.includes(tool)" class="slide">
            <span>Strength</span>
            <input type="range" min="1" max="100" :value="Math.round((brush.strength ?? 0.5) * 100)" aria-label="Strength" @input="emit('brush', { strength: $event.target.value / 100 })" />
            <span class="num">{{ Math.round((brush.strength ?? 0.5) * 100) }}%</span>
          </label>
          <div v-if="tool === 'dodge' || tool === 'burn'" class="seg">
            <button v-for="r in ['shadows', 'midtones', 'highlights']" :key="r" type="button" :class="{ on: (brush.range || 'midtones') === r }" @click="emit('brush', { range: r })">{{ r }}</button>
          </div>
          <div v-if="tool === 'sponge'" class="seg">
            <button type="button" :class="{ on: brush.mode !== 'desaturate' }" @click="emit('brush', { mode: 'saturate' })">Saturate</button>
            <button type="button" :class="{ on: brush.mode === 'desaturate' }" @click="emit('brush', { mode: 'desaturate' })">Desaturate</button>
          </div>
          <template v-if="PAINTS.includes(tool)">
            <label class="pick">
              <input type="checkbox" :checked="!!brush.pressureSize" @change="emit('brush', { pressureSize: $event.target.checked })" />
              <span class="wide">Pen pressure sets the size</span>
            </label>
            <label class="pick">
              <input type="checkbox" :checked="!!brush.pressureOpacity" @change="emit('brush', { pressureOpacity: $event.target.checked })" />
              <span class="wide">Pen pressure sets the opacity</span>
            </label>
          </template>
          <p v-if="HINTS[tool]" class="dim">{{ HINTS[tool] }}</p>
        </div>

        <div v-else-if="tool === 'wand'" class="block">
          <h3>Magic wand</h3>
          <label class="slide">
            <span>Tolerance</span>
            <input type="range" min="0" max="255" :value="tolerance" aria-label="Magic wand tolerance" @input="emit('tolerance', Number($event.target.value))" />
            <span class="num">{{ tolerance }}</span>
          </label>
        </div>

        <div v-else-if="tool === 'gradient'" class="block">
          <h3>Gradient</h3>
          <div class="seg">
            <button type="button" :class="{ on: gradient.kind !== 'radial' }" @click="emit('gradient', { ...gradient, kind: 'linear' })">Linear</button>
            <button type="button" :class="{ on: gradient.kind === 'radial' }" @click="emit('gradient', { ...gradient, kind: 'radial' })">Radial</button>
            <button type="button" title="From the primary to the secondary colour" @click="emit('gradient', { ...gradient, stops: [{ at: 0, color: primary }, { at: 1, color: secondary }] })">From colours</button>
          </div>
          <div class="ramp" :style="{ background: stopsCss }"></div>
          <ul class="stops">
            <li v-for="(s, i) in gradient.stops" :key="i">
              <input type="color" :value="s.color" :aria-label="`Stop ${i + 1} colour`" @change="setStop(i, { color: $event.target.value })" />
              <input type="range" min="0" max="100" :value="Math.round(s.at * 100)" :aria-label="`Stop ${i + 1} position`" @input="setStop(i, { at: $event.target.value / 100 })" />
              <span class="num">{{ Math.round(s.at * 100) }}%</span>
              <button type="button" class="x" :disabled="gradient.stops.length <= 2" aria-label="Remove stop" @click="removeStop(i)">×</button>
            </li>
          </ul>
          <button type="button" class="small" @click="addStop">Add a stop</button>
          <p class="dim">Drag across the picture. It goes into the chosen picture layer (inside the selection), or a new layer.</p>
        </div>

        <div v-if="showsShape" class="block">
          <h3>{{ tool === 'pen' ? 'Pen' : 'Shape' }}</h3>
          <div v-if="tool === 'shape'" class="seg">
            <button v-for="[k, label] in SHAPE_KINDS" :key="k" type="button" :class="{ on: shape.kind === k }" @click="emit('shape', { ...shape, kind: k })">{{ label }}</button>
          </div>
          <label class="pick">
            <input type="checkbox" :checked="shape.fillOn" @change="emit('shape', { ...shape, fillOn: $event.target.checked })" />
            <span>Fill</span>
            <input type="color" :value="shape.fill" aria-label="Fill colour" @change="emit('shape', { ...shape, fill: $event.target.value })" />
          </label>
          <label class="pick">
            <input type="checkbox" :checked="shape.strokeOn" @change="emit('shape', { ...shape, strokeOn: $event.target.checked })" />
            <span>Stroke</span>
            <input type="color" :value="shape.stroke" aria-label="Stroke colour" @change="emit('shape', { ...shape, stroke: $event.target.value })" />
            <input type="number" min="0" max="200" :value="shape.width" aria-label="Stroke width" @change="emit('shape', { ...shape, width: Number($event.target.value) })" />
            <span class="dim">px</span>
          </label>
          <p class="dim">{{ tool === 'pen' ? HINTS.pen : shape.kind === 'polygon' ? 'Click the corners; double-click, Enter or the first point finishes.' : 'Drag on the picture (Shift for a square or circle). It lands on a new layer.' }}</p>
        </div>

        <p v-if="!BRUSHY.includes(tool) && !['wand', 'gradient', 'shape', 'pen'].includes(tool) && HINTS[tool]" class="dim hint">{{ HINTS[tool] }}</p>
        <p v-if="tool === 'text'" class="dim hint">Drag a box with the text tool for paragraph text that wraps.</p>

        <!-- a text layer's type -->
        <div v-if="textLayer" class="block">
          <h3>Type · {{ textLayer.name }}</h3>
          <label class="pick">
            <span>Font</span>
            <GlassSelect
              :model-value="textLayer.text.font"
              aria-label="Font"
              :options="[...(FONTS.includes(textLayer.text.font) ? [] : [{ value: textLayer.text.font, label: textLayer.text.font }]), ...FONTS.map((f, i) => ({ value: f, label: FONT_NAMES[i] }))]"
              @change="(v) => setText({ font: v })"
            />
          </label>
          <div class="grid">
            <label><span>Size</span><input type="number" min="4" max="2000" :value="textLayer.text.size" @change="setText({ size: Number($event.target.value) || 32 })" /></label>
            <label><span>Weight</span><input type="number" min="100" max="900" step="100" :value="textLayer.text.weight || 400" @change="setText({ weight: Number($event.target.value) || 400 })" /></label>
            <label><span>Colour</span><input type="color" :value="textLayer.text.color" @change="setText({ color: $event.target.value })" /></label>
          </div>
          <div class="seg" role="radiogroup" aria-label="Alignment">
            <button v-for="a in ['left', 'center', 'right', 'justify']" :key="a" type="button" :class="{ on: (box?.align || 'left') === a }" :aria-pressed="(box?.align || 'left') === a" @click="setBox({ align: a })">{{ a }}</button>
          </div>
          <div class="grid">
            <label><span>Line height</span><input type="number" min="0.5" max="5" step="0.05" :value="box?.lineHeight ?? 1.2" @change="setBox({ lineHeight: Number($event.target.value) || 1.2 })" /></label>
            <label><span>Tracking</span><input type="number" min="-200" max="1000" step="1" :value="box?.tracking ?? 0" @change="setBox({ tracking: Number($event.target.value) || 0 })" /></label>
            <label><span>Box width</span><input type="number" min="4" max="20000" :value="box?.w ?? ''" placeholder="none" @change="Number($event.target.value) >= 4 ? setBox({ w: Number($event.target.value) }) : emit('text', { textBox: null }, true)" /></label>
          </div>
          <p class="dim">{{ box ? 'Paragraph text: it wraps inside its box.' : 'Point text: give it a box width to make it wrap.' }}</p>
        </div>


        <!-- the chosen adjustment layer, or the document's own sliders -->
        <div class="block">
          <h3 class="with-icon">
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 4h16v16H4ZM6 18c4 0 4-12 12-12" /></svg>
            {{ title }}
            <span v-if="adjustment && layer" class="dim">· {{ layer.name }}</span>
          </h3>
          <CurvesEditor v-if="adjustment?.type === 'curves'" :params="adjustment.params" :histogram="histogram" @update="(p, f) => emit('adjustment', p, f)" />
          <LevelsEditor v-else-if="adjustment?.type === 'levels'" :params="adjustment.params" :histogram="histogram" @update="(p, f) => emit('adjustment', p, f)" />
          <HslEditor v-else-if="adjustment?.type === 'hsl'" :params="adjustment.params" @update="(p, f) => emit('adjustment', p, f)" />
          <div v-else-if="specType" class="spec">
            <p v-if="!fields.length" class="dim">Nothing to set - it works on everything under it.</p>
            <label v-for="f in fields" :key="f.key" :class="f.type ? 'pick' : 'slide'">
              <template v-if="f.type === 'check'">
                <input type="checkbox" :checked="!!specValue(f)" @change="setSpec(f, $event.target.checked, true)" />
                <span class="wide">{{ f.label }}</span>
              </template>
              <template v-else-if="f.type === 'color'">
                <span>{{ f.label }}</span>
                <input type="color" :value="specValue(f)" :aria-label="f.label" @input="setSpec(f, $event.target.value, false)" @change="setSpec(f, $event.target.value, true)" />
              </template>
              <template v-else-if="f.type === 'select'">
                <span>{{ f.label }}</span>
                <GlassSelect :model-value="specValue(f)" :aria-label="f.label" :options="f.options" @change="(v) => setSpec(f, v, true)" />
              </template>
              <template v-else>
                <span>{{ f.label }}</span>
                <input type="range" :min="f.min" :max="f.max" :step="f.step" :value="specValue(f)" :aria-label="f.label" @input="setSpec(f, $event.target.value, false)" @change="setSpec(f, $event.target.value, true)" />
                <span class="num">{{ specValue(f) }}</span>
              </template>
            </label>
          </div>

          <AdjustPanel
            v-else-if="adjustment?.type === 'adjust'"
            bare
            title="EIGHT SLIDERS"
            :adjust="adjustment.params"
            @update="(k, v, f) => emit('adjustment', { ...adjustment.params, [k]: v }, f)"
            @reset="emit('adjustment', Object.fromEntries(Object.keys(adjustment.params).map((k) => [k, 0])), true)"
          />
          <AdjustPanel v-else bare title="WHOLE PICTURE" :adjust="docAdjust" @update="(k, v, f) => emit('adjust', k, v, f)" @reset="emit('reset-adjust')" />
        </div>
      </template>
    </div>
  </section>
</template>

<style scoped>
.props {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
  padding: 12px 12px 10px;
  border: 1px solid var(--line, rgba(190, 170, 255, 0.3));
  border-radius: 16px;
  background: var(--panel-background, rgba(24, 19, 66, 0.72));
  backdrop-filter: blur(var(--glass-blur, 8px));
  color: var(--text);
}

header {
  display: flex;
  align-items: center;
}

h2 {
  flex: 1;
  margin: 0;
  color: var(--accent);
  font-size: 13px;
  font-weight: 600;
  letter-spacing: 0.22em;
}

h3 {
  margin: 0;
  font-size: 13.5px;
  font-weight: 500;
}

.with-icon {
  display: flex;
  align-items: center;
  gap: 8px;
}

.with-icon svg {
  width: 18px;
  height: 18px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.6;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.body {
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding-right: 2px;
  overflow-y: auto;
}

.block {
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
  width: 60px;
}

.slide input {
  flex: 1;
  min-width: 60px;
  accent-color: var(--accent);
}

.num {
  min-width: 38px;
  color: var(--text-dim);
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  text-align: right;
}

.grid {
  display: grid;
  grid-template-columns: repeat(3, 1fr);
  gap: 6px;
}

.grid label {
  display: flex;
  flex-direction: column;
  gap: 2px;
  color: var(--text-dim);
  font-size: 11px;
}

input[type='number'] {
  width: 100%;
  min-width: 0;
  padding: 3px 6px;
  border: 1px solid var(--line);
  border-radius: 6px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 12px;
}

.seg {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
}

.seg button,
.small {
  padding: 3px 10px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: none;
  color: var(--text-dim);
  font-size: 12px;
}

.seg button.on {
  border-color: var(--accent);
  color: var(--text);
  background: color-mix(in srgb, var(--accent) 20%, transparent);
}

.small {
  align-self: flex-start;
}

.ramp {
  height: 14px;
  border: 1px solid var(--line);
  border-radius: 4px;
}

.stops {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.stops li,
.pick {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 12.5px;
}

.stops input[type='range'] {
  flex: 1;
  accent-color: var(--accent);
}

input[type='color'] {
  width: 28px;
  height: 22px;
  padding: 0;
  border: 1px solid var(--line);
  border-radius: 4px;
  background: none;
}

.pick input[type='number'] {
  width: 64px;
}

.pick input[type='checkbox'] {
  accent-color: var(--accent);
}

.pick span:not(.dim) {
  width: 46px;
}

.presets {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 4px;
}

.preset {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 8px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: none;
  color: var(--text-dim);
  font-size: 12px;
  text-align: left;
}

.preset.on {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 18%, transparent);
  color: var(--text);
}

.dot {
  flex: none;
  width: 14px;
  height: 14px;
  border-radius: 50%;
}

.dot.soft {
  background: radial-gradient(circle, #fff 0, transparent 70%);
}

.dot.hard {
  background: #fff;
}

.dot.airbrush {
  background: radial-gradient(circle, rgba(255, 255, 255, 0.6) 0, transparent 70%);
  box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.25);
}

.dot.texture {
  background: radial-gradient(circle, #fff 1px, transparent 1.5px) 0 0 / 4px 4px;
}

.pick .wide {
  width: auto !important;
}

.spec {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.spec .slide span:first-child {
  width: 96px;
}

select {
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

.x {
  padding: 0 4px;
  border: 0;
  background: none;
  color: var(--text-dim);
  font-size: 18px;
  line-height: 1;
}

.dim {
  margin: 0;
  color: var(--text-dim);
  font-size: 11.5px;
}

button {
  font: inherit;
  cursor: pointer;
}

button:disabled {
  opacity: 0.4;
  cursor: default;
}

button:focus-visible,
input:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}
</style>
