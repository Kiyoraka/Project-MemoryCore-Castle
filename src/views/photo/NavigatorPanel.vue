<script setup>
import { computed, ref } from 'vue'
import { viewRect } from './viewHelpers.js'

// PHOTO v3's NAVIGATOR: the whole picture small, with the part the canvas shows as a box. Drag the box (or
// click anywhere) to move there; the slider zooms.
const props = defineProps({
  thumb: { type: String, default: '' }, // data URL of the picture, small
  width: { type: Number, default: 1 },
  height: { type: Number, default: 1 },
  view: { type: Object, default: null }, // { stageW, stageH, scale, x, y }
})
const emit = defineEmits(['center', 'zoom'])

const frame = ref(null)
const dragging = ref(false)

const box = computed(() => {
  if (!props.view) return null
  const r = viewRect(props.view, props.width, props.height)
  return { left: `${(r.x / props.width) * 100}%`, top: `${(r.y / props.height) * 100}%`, width: `${(r.w / props.width) * 100}%`, height: `${(r.h / props.height) * 100}%` }
})
const percent = computed(() => Math.round((props.view?.scale || 1) * 100))

function centreAt(event) {
  const img = frame.value?.querySelector('img')
  if (!img) return
  const r = img.getBoundingClientRect()
  const x = ((event.clientX - r.left) / r.width) * props.width
  const y = ((event.clientY - r.top) / r.height) * props.height
  emit('center', { x: Math.max(0, Math.min(props.width, x)), y: Math.max(0, Math.min(props.height, y)) })
}

function down(event) {
  dragging.value = true
  event.currentTarget.setPointerCapture?.(event.pointerId)
  centreAt(event)
}

function move(event) {
  if (dragging.value) centreAt(event)
}
</script>

<template>
  <section class="navigator" aria-label="Navigator">
    <p v-if="!thumb" class="dim">Open a photo to see it here.</p>
    <template v-else>
      <div ref="frame" class="frame" @pointerdown="down" @pointermove="move" @pointerup="dragging = false" @pointercancel="dragging = false">
        <div class="pic">
          <img :src="thumb" alt="The whole picture" draggable="false" />
          <div v-if="box" class="box" :style="box"></div>
        </div>
      </div>
      <label class="zoom">
        <button type="button" aria-label="Zoom out" @click="emit('zoom', 0.8)">−</button>
        <input type="range" min="-3" max="3" step="0.05" :value="Math.log2(Math.max(0.01, view?.scale || 1))" aria-label="Zoom" @input="emit('zoom', Math.pow(2, Number($event.target.value)) / (view?.scale || 1))" />
        <button type="button" aria-label="Zoom in" @click="emit('zoom', 1.25)">+</button>
        <span class="num">{{ percent }}%</span>
      </label>
    </template>
  </section>
</template>

<style scoped>
.navigator {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
  padding: 10px 12px;
}

.frame {
  display: grid;
  flex: 1;
  place-items: center;
  min-height: 0;
  cursor: crosshair;
  touch-action: none;
}

.pic {
  position: relative;
  overflow: hidden;
  max-width: 100%;
  max-height: 100%;
  line-height: 0;
}

img {
  max-width: 100%;
  max-height: 150px;
  border-radius: 4px;
  user-select: none;
}

.box {
  position: absolute;
  border: 2px solid var(--accent);
  border-radius: 2px;
  box-shadow: 0 0 0 999px rgba(6, 3, 20, 0.35);
  pointer-events: none;
}

.zoom {
  display: flex;
  align-items: center;
  gap: 6px;
  color: var(--text-dim);
  font-size: 12px;
}

.zoom input {
  flex: 1;
  accent-color: var(--accent);
}

.zoom button {
  width: 22px;
  height: 22px;
  padding: 0;
  border: 1px solid var(--line);
  border-radius: 6px;
  background: none;
  color: var(--text);
  cursor: pointer;
}

.num {
  width: 40px;
  text-align: right;
  font-variant-numeric: tabular-nums;
}

.dim {
  margin: 0;
  color: var(--text-dim);
  font-size: 12px;
}
</style>
