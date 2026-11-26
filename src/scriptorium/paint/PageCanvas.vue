<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, toRaw, watch } from 'vue'
import { GlyphCache, ImageCache, collectLinks, collectNeeds, hitLink, paintList } from './painter.js'
import { TileCache, clampZoom, inflate, intersection, pageLayout, pageToScreen, scaleKey, screenToPage, tilesFor, visibleItems, worldBounds, zoomIn, zoomOut } from './tiles.js'

// One page of a Scriptorium document, painted from the Rust display list (painter.js). The canvas is
// the size of the view; the page behind it is painted in device-pixel tiles (zoom x dpr), kept in an
// LRU cache and composed every frame, so it stays crisp at any zoom without a page-sized canvas.
// A scroller over the canvas gives native scrollbars and turns pointer events into page points.
const props = defineProps({
  // the display list: {w, h, items, bg?, bounds?} (or its JSON text); pass it raw (shallowRef/markRaw)
  list: { type: [Object, String], default: null },
  // (font, gids) => glyphs_json (string or object), may be async
  loadGlyphs: { type: Function, default: null },
  // (id) => image_json (string or object), may be async
  loadImage: { type: Function, default: null },
  zoom: { type: Number, default: 1 },
  dpr: { type: Number, default: () => (typeof window !== 'undefined' && window.devicePixelRatio) || 1 },
  tileSize: { type: Number, default: 512 },
  // css px of desk around the page
  pad: { type: Number, default: 24 },
  cacheBytes: { type: Number, default: 192 * 1024 * 1024 },
})
const emit = defineEmits(['link', 'pointer', 'update:zoom'])

const TILES_PER_FRAME = 6
const PREVIEW_PX = 1024

const root = ref(null)
const canvas = ref(null)
const scroller = ref(null)
const viewW = ref(0)
const viewH = ref(0)
const cursor = ref('')

const page = computed(() => {
  const l = props.list
  if (!l) return null
  try {
    return typeof l === 'string' ? JSON.parse(l) : toRaw(l)
  } catch {
    return null
  }
})
const zoomNow = computed(() => clampZoom(props.zoom))
const content = computed(() => {
  const l = page.value
  if (!l) return { w: 0, h: 0 }
  const L = pageLayout({ pageW: +l.w || 0, pageH: +l.h || 0, zoom: zoomNow.value, viewW: viewW.value, viewH: viewH.value, pad: props.pad })
  return { w: L.contentW, h: L.contentH }
})

// --- painting state (deliberately not reactive) ---
let glyphs = new GlyphCache({ loadGlyphs: props.loadGlyphs })
let images = new ImageCache({ loadImage: props.loadImage })
const tiles = new TileCache({ budget: props.cacheBytes })
let pathCache = new WeakMap()
let bounds = []
let links = []
let ready = false
let generation = 0
let preview = null
let raf = 0
let resizeObs = null
let zoomAnchor = null

function makeCanvas(w, h) {
  if (typeof OffscreenCanvas !== 'undefined') return new OffscreenCanvas(w, h)
  if (typeof document === 'undefined') return null
  const c = document.createElement('canvas')
  c.width = w
  c.height = h
  return c
}

function freeze(c) {
  return typeof c.transferToImageBitmap === 'function' ? c.transferToImageBitmap() : c
}

function dprNow() {
  return props.dpr > 0 ? props.dpr : 1
}

function layoutNow() {
  const l = page.value
  const sc = scroller.value
  return pageLayout({
    pageW: +l?.w || 0,
    pageH: +l?.h || 0,
    zoom: zoomNow.value,
    viewW: sc ? sc.clientWidth : viewW.value,
    viewH: sc ? sc.clientHeight : viewH.value,
    scrollX: sc ? sc.scrollLeft : 0,
    scrollY: sc ? sc.scrollTop : 0,
    pad: props.pad,
  })
}

function schedule() {
  if (raf || typeof requestAnimationFrame === 'undefined') return
  raf = requestAnimationFrame(frame)
}

function dropPictures() {
  tiles.clear()
  if (preview && typeof preview.close === 'function') preview.close()
  preview = null
}

async function prepare(l) {
  const gen = ++generation
  ready = false
  dropPictures()
  pathCache = new WeakMap()
  bounds = []
  links = []
  if (!l) return schedule()
  const n = l.items?.length || 0
  bounds = Array.isArray(l.bounds) && l.bounds.length === n ? l.bounds : worldBounds(l)
  links = collectLinks(l)
  schedule()
  const needs = collectNeeds(l)
  try {
    await Promise.all([...[...needs.glyphs].map(([font, gids]) => glyphs.ensure(font, [...gids])), images.ensure([...needs.images])])
  } catch (e) {
    console.warn('[scriptorium] page resources did not all load:', e)
  }
  if (gen !== generation) return
  ready = true
  schedule()
}

function renderTile(l, t) {
  const c = makeCanvas(t.px.w, t.px.h)
  if (!c) return null
  const ctx = c.getContext('2d')
  const idx = visibleItems(bounds, inflate(t.rect, 1 / t.scale), l.items)
  paintList(ctx, l, { glyphs, images, scale: t.scale, viewport: t.rect, indices: idx, pathCache, background: l.bg || '#ffffff' })
  const value = freeze(c)
  tiles.set(t.key, value, t.px.w * t.px.h * 4)
  return value
}

function makePreview(l) {
  const s = Math.min(1, PREVIEW_PX / Math.max(l.w, l.h))
  const c = makeCanvas(Math.max(1, Math.ceil(l.w * s)), Math.max(1, Math.ceil(l.h * s)))
  if (!c) return
  paintList(c.getContext('2d'), l, { glyphs, images, scale: s, pathCache, background: l.bg || '#ffffff' })
  preview = freeze(c)
}

function frame() {
  raf = 0
  const cv = canvas.value
  const sc = scroller.value
  if (!cv || !sc) return
  const dpr = dprNow()
  const vw = sc.clientWidth
  const vh = sc.clientHeight
  const bw = Math.max(1, Math.round(vw * dpr))
  const bh = Math.max(1, Math.round(vh * dpr))
  if (cv.width !== bw) cv.width = bw
  if (cv.height !== bh) cv.height = bh
  cv.style.width = `${vw}px`
  cv.style.height = `${vh}px`
  const ctx = cv.getContext('2d')
  if (!ctx) return
  ctx.setTransform(1, 0, 0, 1, 0, 0)
  ctx.clearRect(0, 0, bw, bh)

  const l = page.value
  if (!l || !(l.w > 0) || !(l.h > 0)) return
  const zoom = zoomNow.value
  const scale = zoom * dpr
  const L = layoutNow()
  // the page's device origin, on whole pixels so the tiles land on the pixel grid
  const ox = Math.round(L.left * dpr)
  const oy = Math.round(L.top * dpr)
  const pw = l.w * scale
  const ph = l.h * scale

  ctx.save()
  ctx.shadowColor = 'rgba(0, 0, 0, 0.18)'
  ctx.shadowBlur = 12 * dpr
  ctx.shadowOffsetY = 2 * dpr
  ctx.fillStyle = l.bg || '#ffffff'
  ctx.fillRect(ox, oy, pw, ph)
  ctx.restore()
  if (!ready) return

  ctx.imageSmoothingEnabled = true
  if (preview) ctx.drawImage(preview, ox, oy, pw, ph)
  const vis = intersection({ x: -ox / scale, y: -oy / scale, w: bw / scale, h: bh / scale }, { x: 0, y: 0, w: l.w, h: l.h })
  if (!vis || vis.w <= 0 || vis.h <= 0) return

  const fits = pw <= bw && ph <= bh
  const want = fits
    ? [{ key: `page/${scaleKey(scale)}`, scale, rect: { x: 0, y: 0, w: l.w, h: l.h }, px: { x: 0, y: 0, w: Math.max(1, Math.ceil(pw)), h: Math.max(1, Math.ceil(ph)) } }]
    : tilesFor(vis, zoom, props.tileSize, dpr, { w: l.w, h: l.h })
  let budget = TILES_PER_FRAME
  let more = false
  for (const t of want) {
    let img = tiles.get(t.key)
    if (!img) {
      if (budget > 0) {
        img = renderTile(l, t)
        budget--
      } else {
        more = true
      }
    }
    if (img) ctx.drawImage(img, ox + t.px.x, oy + t.px.y)
  }
  if (more) schedule()
  else if (!preview) makePreview(l)
}

// --- pointer, in page points ---
function eventPoint(e) {
  const sc = scroller.value
  if (!sc) return null
  const r = sc.getBoundingClientRect()
  const cx = e.clientX - r.left - sc.clientLeft
  const cy = e.clientY - r.top - sc.clientTop
  if (cx < 0 || cy < 0 || cx > sc.clientWidth || cy > sc.clientHeight) return null // on a scrollbar
  return { cx, cy, ...screenToPage(cx, cy, layoutNow()) }
}

function onPointer(e) {
  const p = eventPoint(e)
  if (!p) return
  const l = page.value
  if (e.type === 'pointermove') cursor.value = hitLink(links, p.x, p.y) ? 'pointer' : ''
  emit('pointer', {
    type: e.type.replace(/^pointer/, ''),
    x: p.x,
    y: p.y,
    inside: !!l && p.x >= 0 && p.y >= 0 && p.x <= l.w && p.y <= l.h,
    button: e.button,
    buttons: e.buttons,
    pointerId: e.pointerId,
    pointerType: e.pointerType,
    shiftKey: e.shiftKey,
    ctrlKey: e.ctrlKey,
    altKey: e.altKey,
    metaKey: e.metaKey,
    event: e,
  })
}

function onLeave(e) {
  cursor.value = ''
  emit('pointer', { type: 'leave', x: NaN, y: NaN, inside: false, button: e.button, buttons: e.buttons, pointerId: e.pointerId, pointerType: e.pointerType, event: e })
}

function onClick(e) {
  const p = eventPoint(e)
  if (!p) return
  const hit = hitLink(links, p.x, p.y)
  if (hit) emit('link', { target: hit.target, rect: hit.rect, x: p.x, y: p.y, event: e })
}

function onWheel(e) {
  if (!(e.ctrlKey || e.metaKey)) return
  e.preventDefault()
  const p = eventPoint(e)
  const next = e.deltaY < 0 ? zoomIn(zoomNow.value) : zoomOut(zoomNow.value)
  if (next === zoomNow.value) return
  zoomAnchor = p ? { cx: p.cx, cy: p.cy } : null
  emit('update:zoom', next)
}

// --- reacting ---
watch(page, (l) => prepare(l), { immediate: true })

watch(
  () => [props.loadGlyphs, props.loadImage],
  () => {
    glyphs.clear()
    images.clear()
    glyphs = new GlyphCache({ loadGlyphs: props.loadGlyphs })
    images = new ImageCache({ loadImage: props.loadImage })
    prepare(page.value)
  },
)

// keep the point under the pointer (ctrl+wheel) or the view's centre where it was across a zoom
watch(zoomNow, (z, old) => {
  const sc = scroller.value
  const l = page.value
  if (!sc || !l || !old) return schedule()
  const vw = sc.clientWidth, vh = sc.clientHeight
  const a = zoomAnchor || { cx: vw / 2, cy: vh / 2 }
  zoomAnchor = null
  const before = pageLayout({ pageW: l.w, pageH: l.h, zoom: old, viewW: vw, viewH: vh, scrollX: sc.scrollLeft, scrollY: sc.scrollTop, pad: props.pad })
  const p = screenToPage(a.cx, a.cy, before)
  nextTick(() => {
    const at0 = pageLayout({ pageW: l.w, pageH: l.h, zoom: z, viewW: sc.clientWidth, viewH: sc.clientHeight, pad: props.pad })
    const s = pageToScreen(p.x, p.y, at0)
    sc.scrollLeft = Math.max(0, s.x - a.cx)
    sc.scrollTop = Math.max(0, s.y - a.cy)
    schedule()
  })
})

watch(
  () => props.tileSize,
  () => {
    tiles.clear()
    schedule()
  },
)
watch(() => props.dpr, schedule)
watch(
  () => props.cacheBytes,
  (b) => {
    tiles.budget = b
    tiles.evict()
  },
)

onMounted(() => {
  const sc = scroller.value
  viewW.value = sc.clientWidth
  viewH.value = sc.clientHeight
  if (typeof ResizeObserver !== 'undefined') {
    resizeObs = new ResizeObserver(() => {
      viewW.value = sc.clientWidth
      viewH.value = sc.clientHeight
      schedule()
    })
    resizeObs.observe(sc)
  }
  schedule()
})

onBeforeUnmount(() => {
  generation++
  if (raf && typeof cancelAnimationFrame !== 'undefined') cancelAnimationFrame(raf)
  raf = 0
  resizeObs?.disconnect()
  dropPictures()
  images.clear()
  glyphs.clear()
})

defineExpose({
  /** Drop every painted tile and paint again (after the host changed resources in place). */
  repaint() {
    dropPictures()
    schedule()
  },
  /** Viewport client coordinates -> page points. */
  clientToPage(clientX, clientY) {
    const sc = scroller.value
    if (!sc) return null
    const r = sc.getBoundingClientRect()
    return screenToPage(clientX - r.left - sc.clientLeft, clientY - r.top - sc.clientTop, layoutNow())
  },
  /** Page points -> viewport client coordinates. */
  pageToClient(x, y) {
    const sc = scroller.value
    if (!sc) return null
    const r = sc.getBoundingClientRect()
    const s = pageToScreen(x, y, layoutNow())
    return { x: s.x + r.left + sc.clientLeft, y: s.y + r.top + sc.clientTop }
  },
  /** Scroll so the page point (x, y) sits at the view's (cx, cy) css px (default: its top-left). */
  scrollToPoint(x, y, cx = 0, cy = 0) {
    const sc = scroller.value
    if (!sc) return
    const at0 = pageLayout({ pageW: page.value?.w || 0, pageH: page.value?.h || 0, zoom: zoomNow.value, viewW: sc.clientWidth, viewH: sc.clientHeight, pad: props.pad })
    const s = pageToScreen(x, y, at0)
    sc.scrollLeft = Math.max(0, s.x - cx)
    sc.scrollTop = Math.max(0, s.y - cy)
    schedule()
  },
  layout: layoutNow,
})
</script>

<template>
  <div ref="root" class="page-canvas">
    <canvas ref="canvas" class="paint" aria-hidden="true"></canvas>
    <div
      ref="scroller"
      class="scroller"
      :style="{ cursor }"
      @scroll="schedule"
      @pointerdown="onPointer"
      @pointermove="onPointer"
      @pointerup="onPointer"
      @pointerleave="onLeave"
      @click="onClick"
      @wheel="onWheel"
    >
      <div class="sizer" :style="{ width: `${content.w}px`, height: `${content.h}px` }"></div>
    </div>
  </div>
</template>

<style scoped>
.page-canvas {
  position: relative;
  width: 100%;
  height: 100%;
  min-height: 0;
  overflow: hidden;
  background: var(--scriptorium-desk, #e6e6ea);
}
.paint {
  position: absolute;
  left: 0;
  top: 0;
  display: block;
  pointer-events: none;
}
.scroller {
  position: absolute;
  inset: 0;
  overflow: auto;
  touch-action: pan-x pan-y;
}
.sizer {
  display: block;
}
</style>
