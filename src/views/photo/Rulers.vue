<script setup>
import { computed, ref } from 'vue'
import { guideAxis, rulerTicks } from './viewHelpers.js'

// PHOTO v3's rulers and guides (photo-v3-1): a ruler along the top and the left in pixels, following the
// zoom and the pan. Drag out of the top ruler for a horizontal guide, out of the left one for a vertical
// guide; guides show cyan over the picture; with the move tool a guide drags to a new place, and dragged
// back onto a ruler it goes. Also draws the grid and the artboards' frames and names over the picture.
// The overlay never takes the pointer except on a guide's thin grip.
const props = defineProps({
  view: { type: Object, default: null }, // { stageW, stageH, scale, x, y } - the board on the stage
  width: { type: Number, default: 1 },
  height: { type: Number, default: 1 },
  guides: { type: Array, default: () => [] },
  artboards: { type: Array, default: () => [] },
  grid: { type: Object, default: () => ({ size: 50, show: false }) },
  rulers: { type: Boolean, default: true },
  showGuides: { type: Boolean, default: true },
  movable: { type: Boolean, default: true }, // the move tool is chosen and the guides are not locked
  size: { type: Number, default: 20 },
})
const emit = defineEmits(['add', 'move', 'remove'])

const stage = ref(null)
const drag = ref(null) // { axis: 'x' | 'y', index (-1 new), at, out }

const scale = computed(() => props.view?.scale || 1)
const top = computed(() => (props.view ? rulerTicks(scale.value, props.view.x, props.view.stageW) : { ticks: [] }))
const left = computed(() => (props.view ? rulerTicks(scale.value, props.view.y, props.view.stageH) : { ticks: [] }))

const lines = computed(() => {
  if (!props.view || !props.showGuides) return []
  const out = props.guides.map((g, i) => ({ axis: guideAxis(g), at: g.at, index: i }))
  if (drag.value) {
    if (drag.value.index >= 0) out.splice(drag.value.index, 1, { axis: drag.value.axis, at: drag.value.at, index: drag.value.index, live: true, out: drag.value.out })
    else out.push({ axis: drag.value.axis, at: drag.value.at, index: -1, live: true, out: drag.value.out })
  }
  return out.filter((l) => l.axis).map((l) => ({ ...l, pos: (l.axis === 'x' ? props.view.x : props.view.y) + l.at * scale.value }))
})

const board = computed(() => (props.view ? { left: props.view.x, top: props.view.y, width: props.width * scale.value, height: props.height * scale.value } : null))
const gridStyle = computed(() => {
  if (!board.value || !props.grid?.show) return null
  const step = Math.max(2, props.grid.size * scale.value)
  const c = 'rgba(120, 220, 255, 0.28)'
  return {
    left: `${board.value.left}px`,
    top: `${board.value.top}px`,
    width: `${board.value.width}px`,
    height: `${board.value.height}px`,
    backgroundImage: `linear-gradient(to right, ${c} 1px, transparent 1px), linear-gradient(to bottom, ${c} 1px, transparent 1px)`,
    backgroundSize: `${step}px ${step}px`,
  }
})
const boards = computed(() =>
  props.view
    ? props.artboards.map((a) => ({ ...a, style: { left: `${props.view.x + a.x * scale.value}px`, top: `${props.view.y + a.y * scale.value}px`, width: `${a.w * scale.value}px`, height: `${a.h * scale.value}px` } }))
    : [],
)

// a pointer on the stage -> the document coordinate on that axis, and whether it is off the stage (on a ruler)
function docAt(axis, event) {
  const r = stage.value.getBoundingClientRect()
  const screen = axis === 'x' ? event.clientX - r.left : event.clientY - r.top
  const at = (screen - (axis === 'x' ? props.view.x : props.view.y)) / scale.value
  return { at: Math.round(at), out: screen < 0 }
}

function startNew(axis, event) {
  if (!props.view) return
  event.preventDefault()
  event.currentTarget.setPointerCapture?.(event.pointerId)
  drag.value = { axis, index: -1, ...docAt(axis, event) }
}

function startMove(line, event) {
  if (!props.movable || line.index < 0) return
  event.preventDefault()
  event.stopPropagation()
  event.currentTarget.setPointerCapture?.(event.pointerId)
  drag.value = { axis: line.axis, index: line.index, ...docAt(line.axis, event) }
}

function onMove(event) {
  if (!drag.value) return
  drag.value = { ...drag.value, ...docAt(drag.value.axis, event) }
}

function onUp() {
  const d = drag.value
  drag.value = null
  if (!d) return
  if (d.index < 0) {
    if (!d.out) emit('add', d.axis, d.at)
  } else if (d.out) emit('remove', d.index)
  else emit('move', d.index, d.at)
}

const tickStyle = (axis, at) => (axis === 'x' ? { left: `${at}px` } : { top: `${at}px` })
</script>

<template>
  <div class="rulers" :class="{ on: rulers }" :style="{ '--ruler': `${rulers ? size : 0}px` }">
    <template v-if="rulers">
      <div class="corner" aria-hidden="true">px</div>
      <div class="ruler top" title="Drag down for a guide" @pointerdown="startNew('y', $event)" @pointermove="onMove" @pointerup="onUp" @pointercancel="drag = null">
        <template v-for="t in top.ticks" :key="`t${t.label}`">
          <span class="tick major" :style="tickStyle('x', t.at)"><i>{{ t.label }}</i></span>
          <span v-for="(m, k) in t.minor" :key="k" class="tick" :class="{ mid: k === 1 }" :style="tickStyle('x', m)"></span>
        </template>
      </div>
      <div class="ruler left" title="Drag right for a guide" @pointerdown="startNew('x', $event)" @pointermove="onMove" @pointerup="onUp" @pointercancel="drag = null">
        <template v-for="t in left.ticks" :key="`l${t.label}`">
          <span class="tick major" :style="tickStyle('y', t.at)"><i>{{ t.label }}</i></span>
          <span v-for="(m, k) in t.minor" :key="k" class="tick" :class="{ mid: k === 1 }" :style="tickStyle('y', m)"></span>
        </template>
      </div>
    </template>
    <div ref="stage" class="over">
      <div v-if="gridStyle" class="grid" :style="gridStyle"></div>
      <div v-for="a in boards" :key="a.id" class="board" :style="a.style">
        <span>{{ a.name }} · {{ a.w }}×{{ a.h }}</span>
      </div>
      <div
        v-for="l in lines"
        :key="`${l.index}-${l.axis}`"
        class="guide"
        :class="[l.axis === 'x' ? 'v' : 'h', { live: l.live, gone: l.out, grab: movable && l.index >= 0 }]"
        :style="l.axis === 'x' ? { left: `${l.pos}px` } : { top: `${l.pos}px` }"
        :title="movable ? 'Drag to move, onto a ruler to remove' : ''"
        @pointerdown="startMove(l, $event)"
        @pointermove="onMove"
        @pointerup="onUp"
        @pointercancel="drag = null"
      ></div>
      <span v-if="drag" class="readout" :style="drag.axis === 'x' ? { left: `${(view?.x || 0) + drag.at * scale + 6}px`, top: '6px' } : { top: `${(view?.y || 0) + drag.at * scale + 6}px`, left: '6px' }">{{ drag.out ? 'remove' : `${drag.axis === 'x' ? 'X' : 'Y'} ${drag.at}` }}</span>
    </div>
  </div>
</template>

<style scoped>
.rulers {
  position: absolute;
  inset: 0;
  pointer-events: none;
}

.corner,
.ruler {
  position: absolute;
  z-index: 6;
  overflow: hidden;
  background: color-mix(in srgb, var(--panel-background, rgba(24, 19, 66, 0.9)) 92%, #000);
  color: var(--text-dim);
  font-size: 10px;
  pointer-events: auto;
  user-select: none;
}

.corner {
  top: 0;
  left: 0;
  display: grid;
  place-items: center;
  width: var(--ruler);
  height: var(--ruler);
  border-right: 1px solid var(--line);
  border-bottom: 1px solid var(--line);
}

.ruler.top {
  top: 0;
  left: var(--ruler);
  right: 0;
  height: var(--ruler);
  border-bottom: 1px solid var(--line);
  cursor: s-resize;
}

.ruler.left {
  top: var(--ruler);
  left: 0;
  bottom: 0;
  width: var(--ruler);
  border-right: 1px solid var(--line);
  cursor: e-resize;
}

.tick {
  position: absolute;
  background: color-mix(in srgb, var(--text-dim) 70%, transparent);
}

.top .tick {
  bottom: 0;
  width: 1px;
  height: 4px;
}

.top .tick.mid {
  height: 6px;
}

.top .tick.major {
  height: 100%;
}

.top .tick i {
  position: absolute;
  top: 1px;
  left: 3px;
  font-style: normal;
}

.left .tick {
  right: 0;
  width: 4px;
  height: 1px;
}

.left .tick.mid {
  width: 6px;
}

.left .tick.major {
  width: 100%;
}

.left .tick i {
  position: absolute;
  top: 3px;
  left: 2px;
  font-style: normal;
  writing-mode: vertical-rl;
  transform: rotate(180deg);
}

.over {
  position: absolute;
  top: var(--ruler);
  left: var(--ruler);
  right: 0;
  bottom: 0;
  z-index: 4;
  overflow: hidden;
  pointer-events: none;
}

.grid {
  position: absolute;
}

.board {
  position: absolute;
  outline: 1px solid rgba(255, 255, 255, 0.75);
  box-shadow: 0 0 0 1px rgba(0, 0, 0, 0.5);
}

.board span {
  position: absolute;
  bottom: 100%;
  left: 0;
  padding: 1px 4px;
  color: var(--text);
  font-size: 11px;
  white-space: nowrap;
  text-shadow: 0 0 2px #000;
}

.guide {
  position: absolute;
  pointer-events: none;
}

.guide.v {
  top: 0;
  bottom: 0;
  width: 7px;
  margin-left: -3px;
  background: linear-gradient(to right, transparent 3px, #3ee6f0 3px, #3ee6f0 4px, transparent 4px);
}

.guide.h {
  left: 0;
  right: 0;
  height: 7px;
  margin-top: -3px;
  background: linear-gradient(to bottom, transparent 3px, #3ee6f0 3px, #3ee6f0 4px, transparent 4px);
}

.guide.grab {
  pointer-events: auto;
}

.guide.v.grab {
  cursor: ew-resize;
}

.guide.h.grab {
  cursor: ns-resize;
}

.guide.live {
  opacity: 0.9;
}

.guide.gone {
  opacity: 0.3;
}

.readout {
  position: absolute;
  padding: 2px 6px;
  border-radius: 6px;
  background: rgba(10, 30, 40, 0.85);
  color: #3ee6f0;
  font-size: 11px;
  pointer-events: none;
}
</style>
