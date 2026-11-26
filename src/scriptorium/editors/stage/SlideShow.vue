<script setup>
// Stage's slide show: the slides full screen, one at a time, drawn from the same display lists as the
// page (the painter's paintList), the placeholders' prompt text left out. Next by click, the right
// arrow, the down arrow, Space, Enter or Page Down; back by the left or up arrow, Page Up or Backspace;
// a number and Enter jumps; Esc ends. After the last slide a black screen, then the end.
//
// It takes the room's api (display, glyphs, image) and the document; nothing of any host.
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import { GlyphCache, ImageCache, collectNeeds, paintList } from '../../paint/painter.js'
import { letterbox, showClick, showKey, showList } from './show.js'

const props = defineProps({
  api: { type: Object, required: true },
  doc: { type: [Number, String], required: true },
  start: { type: Number, default: 0 },
  count: { type: Number, default: 1 },
})
const emit = defineEmits(['end'])

const root = ref(null)
const canvas = ref(null)
const at = ref(Math.min(Math.max(0, props.start), Math.max(0, props.count - 1)))
const typed = ref('')
const ended = computed(() => at.value >= props.count)

const glyphs = new GlyphCache({ loadGlyphs: (font, gids) => props.api.glyphs(props.doc, font, gids) })
const images = new ImageCache({ loadImage: (id) => props.api.image(props.doc, id) })
const lists = new Map()
let token = 0

async function listFor(i) {
  if (lists.has(i)) return lists.get(i)
  const raw = await props.api.display(props.doc, i)
  const l = showList(raw)
  if (l) lists.set(i, l)
  return l
}

async function ready(l) {
  const needs = collectNeeds(l)
  await Promise.all([...[...needs.glyphs].map(([font, gids]) => glyphs.ensure(font, [...gids])), images.ensure([...needs.images])])
}

async function draw() {
  const c = canvas.value
  if (!c) return
  const mine = ++token
  const dpr = (typeof window !== 'undefined' && window.devicePixelRatio) || 1
  const vw = c.clientWidth || (typeof window !== 'undefined' ? window.innerWidth : 0)
  const vh = c.clientHeight || (typeof window !== 'undefined' ? window.innerHeight : 0)
  c.width = Math.max(1, Math.round(vw * dpr))
  c.height = Math.max(1, Math.round(vh * dpr))
  const ctx = c.getContext('2d')
  if (!ctx) return
  ctx.setTransform(1, 0, 0, 1, 0, 0)
  ctx.fillStyle = '#000'
  ctx.fillRect(0, 0, c.width, c.height)
  if (ended.value) return
  const l = await listFor(at.value).catch(() => null)
  if (!l || mine !== token) return
  await ready(l).catch(() => {})
  if (mine !== token) return
  const box = letterbox(+l.w || 0, +l.h || 0, vw, vh)
  ctx.setTransform(dpr, 0, 0, dpr, box.x * dpr, box.y * dpr)
  paintList(ctx, l, { glyphs, images, scale: box.scale })
  // the next slide waits ready
  if (at.value + 1 < props.count) listFor(at.value + 1).then((n) => n && ready(n)).catch(() => {})
}

function go(r) {
  if (!r) return
  if (r.end) return finish()
  if (r.digit !== undefined) {
    typed.value = r.digit
    return
  }
  typed.value = ''
  if (r.to !== undefined && r.to !== at.value) {
    at.value = r.to
    draw()
  }
}

function onKey(e) {
  const r = showKey(e.key, at.value, props.count, typed.value)
  if (!r) return
  e.preventDefault()
  e.stopPropagation()
  go(r)
}

function onPointer(e) {
  if (e.button !== 0 && e.button !== 2) return
  e.preventDefault()
  go(showClick(e.button, at.value, props.count))
}

let done = false
function finish() {
  if (done) return
  done = true
  if (typeof document !== 'undefined' && document.fullscreenElement && document.exitFullscreen) document.exitFullscreen().catch(() => {})
  emit('end')
}

function onFullscreen() {
  // leaving full screen by the system's own key ends the show too
  if (typeof document !== 'undefined' && !document.fullscreenElement && entered) finish()
}

let entered = false
let resize = null
onMounted(async () => {
  await nextTick()
  root.value?.focus?.()
  const el = root.value
  if (el && typeof el.requestFullscreen === 'function') {
    el.requestFullscreen()
      .then(() => {
        entered = true
      })
      .catch(() => {})
  }
  document.addEventListener('fullscreenchange', onFullscreen)
  resize = () => draw()
  window.addEventListener('resize', resize)
  draw()
})

onBeforeUnmount(() => {
  document.removeEventListener('fullscreenchange', onFullscreen)
  if (resize) window.removeEventListener('resize', resize)
  glyphs.clear()
  images.clear()
})
</script>

<template>
  <div ref="root" class="slide-show" tabindex="0" role="dialog" aria-label="Slide show" @keydown="onKey" @mousedown="onPointer" @contextmenu.prevent>
    <canvas ref="canvas" class="screen"></canvas>
    <div v-if="ended" class="end">End of slide show. Click to leave.</div>
    <div v-else-if="typed" class="jump">Go to slide {{ typed }}</div>
  </div>
</template>

<style scoped>
.slide-show {
  position: fixed;
  inset: 0;
  z-index: 2147483000;
  background: #000;
  outline: none;
  cursor: default;
}
.screen {
  position: absolute;
  inset: 0;
  width: 100%;
  height: 100%;
}
.end {
  position: absolute;
  top: 24px;
  left: 0;
  right: 0;
  text-align: center;
  color: rgba(255, 255, 255, 0.75);
  font: 15px system-ui, sans-serif;
}
.jump {
  position: absolute;
  bottom: 18px;
  right: 22px;
  padding: 6px 12px;
  border-radius: 6px;
  background: rgba(255, 255, 255, 0.14);
  color: #fff;
  font: 14px system-ui, sans-serif;
}
</style>
