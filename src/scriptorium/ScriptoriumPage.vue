<script setup>
// The page area: the painter's PageCanvas (paint/), loaded lazily so the room never waits on it. Until it
// is there - or if it cannot load - a plain white page of the right shape stands in.
//
// Over the canvas: the caret (blinking) and the selection's boxes, placed with the painter's own page
// layout, and a hidden textarea that is the page's keyboard (input.js): its key presses, typing, IME
// compositions and pastes come out as intents for the room. Pointer events come out in page points.
import { computed, defineAsyncComponent, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { attachTypist } from './input.js'
import { caretBox, onPage, toScreen } from './selection.js'

const props = defineProps({
  // the display list as an object ({ w, h, items }), passed raw
  list: { type: Object, default: null },
  loadGlyphs: { type: Function, default: null },
  loadImage: { type: Function, default: null },
  zoom: { type: Number, default: 1 },
  // the page shown (0-based) and the editing ground's caret { page, x, y, height } and boxes [{ page, x, y, w, h }]
  page: { type: Number, default: 0 },
  caret: { type: Object, default: null },
  rects: { type: Array, default: () => [] },
})
const emit = defineEmits(['update:zoom', 'pointer', 'link', 'intent'])

const ready = ref(false)
const failed = ref(false)
const PageCanvas = defineAsyncComponent({
  loader: () =>
    import('./paint/PageCanvas.vue').then((m) => {
      ready.value = true
      return m
    }),
  onError(_error, _retry, fail) {
    failed.value = true
    fail()
  },
})

const painted = computed(() => !failed.value && !!props.list)
const aspect = computed(() => {
  const l = props.list
  return l && l.w > 0 && l.h > 0 ? `${l.w} / ${l.h}` : '595 / 842'
})

// --- the overlay, placed with the painter's layout (read again on scroll, resize and zoom) ---
const root = ref(null)
const canvas = ref(null)
const input = ref(null)
const tick = ref(0)
let raf = 0
function relayout() {
  if (raf || typeof requestAnimationFrame === 'undefined') return
  raf = requestAnimationFrame(() => {
    raf = 0
    tick.value++
  })
}
const layout = computed(() => {
  void tick.value
  const c = canvas.value
  return c && typeof c.layout === 'function' && props.list ? c.layout() : null
})
const bar = computed(() => {
  const c = props.caret
  return layout.value && c && c.page === props.page ? caretBox(c, layout.value) : null
})
const boxes = computed(() => (layout.value ? onPage(props.rects, props.page).map((r) => toScreen(r, layout.value)) : []))
// the blink starts again whenever the caret moves
const blinkKey = computed(() => (bar.value ? `${bar.value.left}:${bar.value.top}` : ''))
// the hidden field sits at the caret, so an IME's candidate window opens beside it
const fieldStyle = computed(() => {
  const b = bar.value
  return b ? { left: `${b.left}px`, top: `${b.top}px`, height: `${b.height}px` } : { left: '0px', top: '0px' }
})

watch(() => [props.list, props.zoom, props.caret, props.rects, ready.value], () => nextTick(relayout))

function onPointer(p) {
  emit('pointer', p)
  // the click moved focus to the page's frame: give it back to the keyboard
  if (p && (p.type === 'up' || p.type === 'down')) nextTick(focusInput)
}

function focusInput() {
  input.value?.focus?.({ preventScroll: true })
}

let detach = null
let resizeObs = null
onMounted(() => {
  if (input.value) detach = attachTypist(input.value, (it) => emit('intent', it))
  if (typeof ResizeObserver !== 'undefined' && root.value) {
    resizeObs = new ResizeObserver(relayout)
    resizeObs.observe(root.value)
  }
})
onBeforeUnmount(() => {
  detach?.()
  resizeObs?.disconnect()
  if (raf && typeof cancelAnimationFrame !== 'undefined') cancelAnimationFrame(raf)
})

defineExpose({ focusInput })
</script>

<template>
  <div ref="root" class="page-area" @scroll.capture="relayout">
    <PageCanvas
      v-if="painted"
      ref="canvas"
      class="canvas"
      :list="list"
      :load-glyphs="loadGlyphs"
      :load-image="loadImage"
      :zoom="zoom"
      @update:zoom="(z) => emit('update:zoom', z)"
      @pointer="onPointer"
      @link="(l) => emit('link', l)"
    />
    <div v-if="!painted || !ready" class="fallback" :aria-busy="!list">
      <div class="sheet" :style="{ aspectRatio: aspect }"></div>
    </div>
    <div class="overlay" aria-hidden="true">
      <div v-for="(b, i) in boxes" :key="i" class="sel" :style="{ left: `${b.left}px`, top: `${b.top}px`, width: `${b.width}px`, height: `${b.height}px` }"></div>
      <div v-if="bar" :key="blinkKey" class="caret" :style="{ left: `${bar.left}px`, top: `${bar.top}px`, height: `${bar.height}px` }"></div>
    </div>
    <textarea
      ref="input"
      class="keys"
      data-scr-input
      :style="fieldStyle"
      aria-label="Type on the page"
      autocomplete="off"
      autocapitalize="off"
      spellcheck="false"
      rows="1"
      tabindex="-1"
    ></textarea>
  </div>
</template>

<style scoped>
.page-area {
  position: relative;
  min-width: 0;
  min-height: 0;
  height: 100%;
  overflow: hidden;
}
.canvas {
  position: absolute;
  inset: 0;
}
.fallback {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  padding: 24px;
  overflow: auto;
}
.sheet {
  height: min(100%, 1100px);
  max-width: 100%;
  background: #fff;
  box-shadow: 0 8px 30px rgba(0, 0, 0, 0.35);
}
.overlay {
  position: absolute;
  inset: 0;
  overflow: hidden;
  pointer-events: none;
}
.sel {
  position: absolute;
  background: rgba(64, 120, 255, 0.28);
  mix-blend-mode: multiply;
}
.caret {
  position: absolute;
  width: 1.5px;
  margin-left: -0.75px;
  background: #111;
  animation: blink 1.06s steps(1, end) infinite;
}
@keyframes blink {
  0%,
  55% {
    opacity: 1;
  }
  56%,
  100% {
    opacity: 0;
  }
}
.keys {
  position: absolute;
  width: 2px;
  min-height: 1px;
  padding: 0;
  border: 0;
  margin: 0;
  opacity: 0;
  resize: none;
  overflow: hidden;
  pointer-events: none;
  white-space: pre;
  caret-color: transparent;
  color: transparent;
  background: transparent;
}
</style>
