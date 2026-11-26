<script setup>
// STUDIO > PAINT (his design at the Keep): one canvas; the tools and a colour wheel on the left,
// LAYERS on the right, the chat under them. The member paints on raster layers (pencil, brush,
// eraser, bucket); the companion draws on SVG layers of its own, each thing on its own named layer,
// its new strokes drawing themselves. The painting lives in composables/usePaint.js, so it survives
// the room being hidden.
import { computed, nextTick, onBeforeUnmount, onMounted, reactive, ref, watch } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import HelpTip from '../HelpTip.vue'
import { paintAsk, paintExport, paintList, paintOpen, paintSave, paintWay } from '../../api/castle.js'
import { ensurePainting, flatten, loadPainting, memberLayer, paint, paintingForSave, refreshThumb, startPainting } from '../../composables/usePaint.js'
import { applyOps, canDelete, companionOf, contextOf, defaultName, insertAbove, moveLayer, nextLayerName, renameLayer, replyLine } from './layers.js'
import { cleanSvg } from './svgclean.js'
import { floodFill, rgbaOf } from './flood.js'
import { strokePlan } from './strokeanim.js'
import { hexToHsv, wheelColour, wheelPoint } from './colour.js'

const props = defineProps({
  companion: { type: String, default: 'Your companion' },
})

ensurePainting()
const who = computed(() => companionOf(props.companion))

// ---------- the tools ----------
const TOOLS = [
  { id: 'pencil', label: 'Pencil', icon: '✎' },
  { id: 'brush', label: 'Brush', icon: '●' },
  { id: 'eraser', label: 'Eraser', icon: '▱' },
  { id: 'bucket', label: 'Bucket', icon: '◧' },
]
const STYLES = ['Anime', 'Watercolour', 'Pixel', 'Crayon']

// ---------- the colour wheel: the current and the last colour ----------
const WHEEL = 150
const wheel = ref(null)
const brightness = ref(Math.round(hexToHsv(paint.colour).v * 100))
const marker = computed(() => {
  const p = wheelPoint(paint.colour, WHEEL / 2)
  return { left: `${WHEEL / 2 + p.x}px`, top: `${WHEEL / 2 + p.y}px` }
})
function drawWheel() {
  const c = wheel.value
  if (!c) return
  const ctx = c.getContext('2d')
  const img = ctx.createImageData(WHEEL, WHEEL)
  const r = WHEEL / 2
  for (let y = 0; y < WHEEL; y++) {
    for (let x = 0; x < WHEEL; x++) {
      const hex = wheelColour(x - r + 0.5, y - r + 0.5, r, brightness.value / 100)
      if (!hex) continue
      const [cr, cg, cb] = rgbaOf(hex)
      img.data.set([cr, cg, cb, 255], (y * WHEEL + x) * 4)
    }
  }
  ctx.putImageData(img, 0, 0)
}
let picking = false
function pickAt(e) {
  const rect = wheel.value.getBoundingClientRect()
  const r = WHEEL / 2
  const dx = ((e.clientX - rect.left) * WHEEL) / rect.width - r
  const dy = ((e.clientY - rect.top) * WHEEL) / rect.height - r
  const dist = Math.hypot(dx, dy)
  const k = dist > r ? r / dist : 1
  paint.colour = wheelColour(dx * k, dy * k, r, brightness.value / 100)
}
function wheelDown(e) {
  if (paint.colour !== paint.last) paint.last = paint.colour
  picking = true
  wheel.value.setPointerCapture(e.pointerId)
  pickAt(e)
}
const wheelMove = (e) => picking && pickAt(e)
const wheelUp = () => (picking = false)
function onBrightness() {
  const { h, s } = hexToHsv(paint.colour)
  const r = WHEEL / 2
  const a = (h * Math.PI) / 180
  paint.colour = wheelColour(Math.cos(a) * s * r, Math.sin(a) * s * r, r, brightness.value / 100)
  drawWheel()
}
function swapColours() {
  ;[paint.colour, paint.last] = [paint.last, paint.colour]
  brightness.value = Math.round(hexToHsv(paint.colour).v * 100)
  drawWheel()
}

// ---------- the canvas ----------
const wrap = ref(null)
const stage = ref(null)
const box = reactive({ w: 0, h: 0 })
const stageStyle = computed(() => {
  const s = Math.min(box.w / paint.width, box.h / paint.height) || 0
  return { width: `${Math.floor(paint.width * s)}px`, height: `${Math.floor(paint.height * s)}px` }
})
let observer
const active = computed(() => paint.layers.find((l) => l.id === paint.active) ?? null)
const canDraw = computed(() => active.value?.owner === 'member' && active.value.visible)
const status = ref('')
const problem = ref('')

// a member layer's canvas lives in usePaint; the room only shows it
function mount(el, layer) {
  if (!el || !layer.canvas || layer.canvas.parentNode === el) return
  Object.assign(layer.canvas.style, { position: 'absolute', inset: '0', width: '100%', height: '100%' })
  el.appendChild(layer.canvas)
}

function changed(layer) {
  refreshThumb(layer)
  paint.dirty = true
}

function point(e) {
  const r = stage.value.getBoundingClientRect()
  return { x: ((e.clientX - r.left) * paint.width) / r.width, y: ((e.clientY - r.top) * paint.height) / r.height }
}

let drawing = null
function down(e) {
  if (e.pointerType === 'mouse' && e.button !== 0) return
  problem.value = ''
  if (!canDraw.value) {
    problem.value = active.value?.owner === 'companion' ? `That layer is ${who.value}'s. Choose one of your layers to paint on.` : 'Show the layer to paint on it.'
    return
  }
  const layer = active.value
  const ctx = layer.canvas.getContext('2d')
  const p = point(e)
  if (paint.tool === 'bucket') {
    const img = ctx.getImageData(0, 0, paint.width, paint.height)
    if (floodFill(img.data, paint.width, paint.height, p.x, p.y, rgbaOf(paint.colour), paint.tolerance)) {
      ctx.putImageData(img, 0, 0)
      changed(layer)
    }
    return
  }
  e.currentTarget.setPointerCapture(e.pointerId)
  ctx.save()
  ctx.globalCompositeOperation = paint.tool === 'eraser' ? 'destination-out' : 'source-over'
  ctx.strokeStyle = ctx.fillStyle = paint.colour
  ctx.lineWidth = paint.tool === 'pencil' ? 2 : paint.size
  ctx.lineCap = 'round'
  ctx.lineJoin = 'round'
  ctx.beginPath()
  ctx.arc(p.x, p.y, ctx.lineWidth / 2, 0, Math.PI * 2)
  ctx.fill()
  drawing = { ctx, last: p, layer }
}
function move(e) {
  if (!drawing) return
  const p = point(e)
  const { ctx, last } = drawing
  ctx.beginPath()
  ctx.moveTo(last.x, last.y)
  ctx.lineTo(p.x, p.y)
  ctx.stroke()
  drawing.last = p
}
function up() {
  if (!drawing) return
  drawing.ctx.restore()
  changed(drawing.layer)
  drawing = null
}

// ---------- LAYERS ----------
const topFirst = computed(() => [...paint.layers].reverse())
const renaming = ref('')
const renameText = ref('')
function addLayer() {
  const layer = memberLayer(nextLayerName(paint.layers))
  paint.layers = insertAbove(paint.layers, layer, paint.active)
  paint.active = layer.id
  refreshThumb(layer)
  paint.dirty = true
}
function toggle(layer) {
  layer.visible = !layer.visible
  paint.dirty = true
}
function shift(layer, dir) {
  paint.layers = moveLayer(paint.layers, layer.id, dir)
  paint.dirty = true
}
function startRename(layer) {
  renaming.value = layer.id
  renameText.value = layer.name
}
function finishRename() {
  if (!renaming.value) return
  paint.layers = renameLayer(paint.layers, renaming.value, renameText.value, who.value)
  renaming.value = ''
  paint.dirty = true
}
function removeLayer(layer) {
  if (!canDelete(paint.layers, layer.id)) return
  paint.layers = paint.layers.filter((l) => l.id !== layer.id)
  delete paint.thumbs[layer.id]
  if (paint.active === layer.id) paint.active = [...paint.layers].reverse().find((l) => l.owner === 'member')?.id ?? ''
  paint.dirty = true
}

// ---------- New, Open, Save, Export ----------
const saveName = ref(paint.name)
const opening = ref(false)
const saved = ref([])
// { kind: 'new' | 'open', name } while the member is asked about unsaved strokes
const pending = ref(null)
async function act(work) {
  problem.value = ''
  status.value = ''
  try {
    await work()
  } catch (e) {
    problem.value = String(e?.message ?? e)
  }
}
function fresh() {
  startPainting()
  saveName.value = ''
  status.value = 'A fresh canvas.'
}
const askNew = () => (paint.dirty ? (pending.value = { kind: 'new' }) : fresh())
const toggleOpen = () =>
  act(async () => {
    opening.value = !opening.value
    if (opening.value) saved.value = await paintList()
  })
const openOne = (name) =>
  act(async () => {
    await loadPainting(await paintOpen(name))
    saveName.value = paint.name
    opening.value = false
    status.value = `Opened ${paint.name}.`
  })
const askOpen = (name) => (paint.dirty ? (pending.value = { kind: 'open', name }) : openOne(name))
function goOn() {
  const p = pending.value
  pending.value = null
  if (p?.kind === 'new') fresh()
  else if (p?.kind === 'open') openOne(p.name)
}
const save = () =>
  act(async () => {
    const name = saveName.value.trim() || defaultName()
    const kept = await paintSave(await paintingForSave(name))
    paint.name = kept
    saveName.value = kept
    paint.dirty = false
    status.value = `Saved as ${kept}.`
    if (opening.value) saved.value = await paintList()
  })
const exportOne = () =>
  act(async () => {
    const path = await paintExport((await flatten()).toDataURL('image/png'), saveName.value.trim() || defaultName())
    if (path) status.value = `Saved to ${path}`
  })

// ---------- the chat ----------
const way = ref(null) // 'claude' | 'key' | '' once read
const draft = ref('')
const lines = ref(null)
const picker = ref(null)
const wayLine = computed(() => (way.value === '' ? `${who.value} draws with Claude Code, or with an OpenAI key in Settings > Keys.` : ''))
const canSend = computed(() => !!draft.value.trim() && !paint.busy && way.value !== '')

// a reference picture, made at most 1600 pixels long so the ask stays light
function readPicture(file) {
  if (!file || !/^image\/(png|jpeg|webp)$/.test(file.type)) {
    problem.value = 'Use a PNG, JPEG or WebP picture.'
    return
  }
  const reader = new FileReader()
  reader.onload = () => {
    const img = new Image()
    img.onload = () => {
      const k = Math.min(1, 1600 / Math.max(img.width, img.height))
      const c = document.createElement('canvas')
      c.width = Math.max(1, Math.round(img.width * k))
      c.height = Math.max(1, Math.round(img.height * k))
      c.getContext('2d').drawImage(img, 0, 0, c.width, c.height)
      paint.reference = { url: file.type === 'image/png' ? c.toDataURL('image/png') : c.toDataURL('image/jpeg', 0.9), name: file.name || 'picture' }
    }
    img.onerror = () => (problem.value = 'That picture could not be read.')
    img.src = reader.result
  }
  reader.readAsDataURL(file)
}
function onPaste(e) {
  const item = [...(e.clipboardData?.items ?? [])].find((i) => i.type.startsWith('image/'))
  if (!item) return
  e.preventDefault()
  readPicture(item.getAsFile())
}
const onDrop = (e) => readPicture(e.dataTransfer?.files?.[0])
function onPick(e) {
  readPicture(e.target.files?.[0])
  e.target.value = ''
}
function dropReference() {
  paint.reference = null
  paint.trace = false
}

function scrollDown() {
  nextTick(() => lines.value && (lines.value.scrollTop = lines.value.scrollHeight))
}

// the companion's new paths draw themselves in order, the fills fading in after
function animate(ids) {
  if (!ids.length || !stage.value || window.matchMedia?.('(prefers-reduced-motion: reduce)').matches) return
  const els = []
  for (const id of ids) {
    const root = stage.value.querySelector(`[data-layer="${id}"] svg`)
    if (root) els.push(...root.querySelectorAll('path, rect, circle, ellipse, line, polyline, polygon'))
  }
  const lengths = els.map((el) => {
    try {
      return el.getTotalLength()
    } catch {
      return 0
    }
  })
  const plan = strokePlan(lengths)
  els.forEach((el, i) => {
    if (!el.animate) return
    const step = plan.steps[i]
    const style = getComputedStyle(el)
    const stroked = style.stroke && style.stroke !== 'none'
    const len = Math.max(1, lengths[i])
    if (stroked) {
      const dash = `${len} ${len}`
      el.animate([{ strokeDasharray: dash, strokeDashoffset: len }, { strokeDasharray: dash, strokeDashoffset: 0 }], { delay: step.delay, duration: step.duration, easing: 'ease-in-out', fill: 'backwards' })
    }
    const opacity = Number(style.fillOpacity) || 1
    el.animate([{ fillOpacity: 0 }, { fillOpacity: opacity }], {
      delay: stroked ? plan.fillDelay : step.delay,
      duration: stroked ? plan.fillDuration : Math.max(200, step.duration),
      easing: 'ease-out',
      fill: 'backwards',
    })
  })
}

async function send() {
  const line = draft.value.trim()
  if (!canSend.value) return
  draft.value = ''
  problem.value = ''
  paint.chat.push({ who: 'member', text: line, ref: paint.reference?.url ?? '' })
  paint.busy = true
  scrollDown()
  try {
    const canvas = (await flatten()).toDataURL('image/png')
    const answer = await paintAsk({
      companion: who.value,
      line,
      style: paint.style,
      width: paint.width,
      height: paint.height,
      canvas,
      reference: paint.reference?.url ?? '',
      layers: contextOf(paint.layers),
    })
    const before = paint.layers.length
    // cleaned again here, each layer's ids given its own prefix
    const result = applyOps(paint.layers, answer.ops, who.value, (svg, id) => cleanSvg(svg, paint.width, paint.height, `${id}-`).svg ?? null)
    paint.layers = result.layers
    for (const id of result.changed) refreshThumb(paint.layers.find((l) => l.id === id))
    if (result.changed.length || paint.layers.length !== before) paint.dirty = true
    paint.chat.push({ who: 'companion', text: replyLine(answer.say, result.dropped) })
    await nextTick()
    animate(result.changed)
  } catch (e) {
    paint.chat.push({ who: 'companion', text: String(e?.message ?? e), problem: true })
  } finally {
    paint.busy = false
    scrollDown()
  }
}

onMounted(async () => {
  drawWheel()
  observer = new ResizeObserver(([entry]) => {
    box.w = entry.contentRect.width
    box.h = entry.contentRect.height
  })
  if (wrap.value) observer.observe(wrap.value)
  scrollDown()
  try {
    way.value = await paintWay()
  } catch {
    way.value = null
  }
})
onBeforeUnmount(() => {
  observer?.disconnect()
  up()
})
watch(() => paint.name, (n) => (saveName.value = n))
</script>

<template>
  <div class="paint">
    <section class="tools widget">
      <h3>TOOLS</h3>
      <div class="tool-grid" role="radiogroup" aria-label="Tool">
        <button v-for="t in TOOLS" :key="t.id" role="radio" :aria-checked="paint.tool === t.id" :class="{ on: paint.tool === t.id }" :title="t.label" @click="paint.tool = t.id">
          <span class="icon" aria-hidden="true">{{ t.icon }}</span>{{ t.label }}
        </button>
      </div>
      <label v-if="paint.tool === 'brush' || paint.tool === 'eraser'" class="slider">
        <span>Size {{ paint.size }}</span>
        <input v-model.number="paint.size" type="range" min="1" max="64" />
      </label>
      <label v-else-if="paint.tool === 'bucket'" class="slider">
        <span>Tolerance {{ paint.tolerance }}</span>
        <input v-model.number="paint.tolerance" type="range" min="0" max="128" />
      </label>
      <h3>COLOUR</h3>
      <div class="wheel-box">
        <canvas ref="wheel" :width="WHEEL" :height="WHEEL" class="wheel" aria-label="Colour wheel" @pointerdown="wheelDown" @pointermove="wheelMove" @pointerup="wheelUp" @pointercancel="wheelUp"></canvas>
        <span class="marker" :style="marker" aria-hidden="true"></span>
      </div>
      <label class="slider">
        <span>Brightness</span>
        <input v-model.number="brightness" type="range" min="0" max="100" @input="onBrightness" />
      </label>
      <div class="swatches">
        <span class="swatch current" :style="{ background: paint.colour }" :title="`Current ${paint.colour}`"></span>
        <button class="swatch last" :style="{ background: paint.last }" :title="`Last colour ${paint.last} - click to swap`" aria-label="Swap with the last colour" @click="swapColours"></button>
        <span class="swatch-words">Current · Last</span>
      </div>
    </section>

    <section class="canvas-box widget">
      <div class="bar">
        <input v-model="saveName" class="name" maxlength="60" placeholder="Name this painting" aria-label="Painting name" />
        <button @click="askNew">New</button>
        <button :class="{ active: opening }" @click="toggleOpen">Open</button>
        <button class="primary" @click="save">Save</button>
        <button @click="exportOne">Export</button>
        <span v-if="paint.dirty" class="dirty" title="Unsaved strokes">●</span>
        <span class="spacer"></span>
        <p class="hint vector">Vector strokes: playful and editable, not photo-real renders.</p>
        <HelpTip :text="`${who} draws as vector strokes on layers of its own, so you can hide, move or ask it to change any one thing. It reads your canvas, never changes your layers, and is not a photo-real renderer.`" />
      </div>
      <div v-if="pending" class="confirm" role="group" aria-label="Unsaved strokes">
        <span>Unsaved strokes will be lost. Go on?</span>
        <button class="danger" @click="goOn">Yes</button>
        <button @click="pending = null">No</button>
      </div>
      <div v-if="opening" class="open-strip">
        <button v-for="s in saved" :key="s.name" class="open-thumb" :title="s.name" @click="askOpen(s.name)">
          <img v-if="s.flat" :src="convertFileSrc(s.flat)" :alt="s.name" loading="lazy" />
          <span>{{ s.name }}</span>
        </button>
        <p v-if="!saved.length" class="empty">No paintings saved yet.</p>
      </div>
      <div ref="wrap" class="wrap">
        <div ref="stage" class="stage" :style="stageStyle">
          <template v-for="(layer, i) in paint.layers" :key="layer.id">
            <div v-if="layer.owner === 'member'" v-show="layer.visible" class="layer" :ref="(el) => mount(el, layer)"></div>
            <div v-else v-show="layer.visible" class="layer svg" :data-layer="layer.id" v-html="layer.svg"></div>
            <!-- the trace: the reference at 30%, locked, just above the bottom layer; never saved -->
            <img v-if="i === 0 && paint.trace && paint.reference" class="layer trace" :src="paint.reference.url" alt="" />
          </template>
          <div class="input" :class="[paint.tool, { blocked: !canDraw }]" @pointerdown="down" @pointermove="move" @pointerup="up" @pointercancel="up"></div>
        </div>
      </div>
      <p v-if="problem" role="alert" class="problem">{{ problem }}</p>
      <p v-else-if="status" class="hint">{{ status }}</p>
    </section>

    <section class="layers widget">
      <header>
        <h3>LAYERS</h3>
        <button @click="addLayer">＋ New layer</button>
      </header>
      <ul>
        <li v-for="(layer, i) in topFirst" :key="layer.id" :class="{ active: layer.id === paint.active, companion: layer.owner === 'companion' }" @click="paint.active = layer.id">
          <button class="eye" :aria-pressed="layer.visible" :title="layer.visible ? 'Hide' : 'Show'" :aria-label="`${layer.visible ? 'Hide' : 'Show'} ${layer.name}`" @click.stop="toggle(layer)">{{ layer.visible ? '◉' : '○' }}</button>
          <img class="thumb" :src="paint.thumbs[layer.id]" alt="" />
          <input v-if="renaming === layer.id" v-model="renameText" class="rename" maxlength="60" aria-label="Layer name" @click.stop @keydown.enter="finishRename" @keydown.esc="renaming = ''" @blur="finishRename" />
          <span v-else class="layer-name" :title="layer.name" @dblclick.stop="startRename(layer)">{{ layer.name }}</span>
          <span class="row-actions">
            <button :disabled="i === 0" title="Up" aria-label="Move up" @click.stop="shift(layer, 1)">▲</button>
            <button :disabled="i === topFirst.length - 1" title="Down" aria-label="Move down" @click.stop="shift(layer, -1)">▼</button>
            <button title="Rename" aria-label="Rename" @click.stop="startRename(layer)">✎</button>
            <button :disabled="!canDelete(paint.layers, layer.id)" :title="canDelete(paint.layers, layer.id) ? 'Delete' : 'Your last layer stays'" aria-label="Delete" @click.stop="removeLayer(layer)">✕</button>
          </span>
        </li>
      </ul>
    </section>

    <section class="chat widget" @dragover.prevent @drop.prevent="onDrop">
      <header>
        <h3>CHAT</h3>
        <span v-if="wayLine" class="problem">{{ wayLine }}</span>
      </header>
      <div ref="lines" class="lines">
        <p v-if="!paint.chat.length" class="empty">Tell {{ who }} what to draw - "a moon over the hills" - or add a picture to follow.</p>
        <div v-for="(m, i) in paint.chat" :key="i" class="line" :class="[m.who, { problem: m.problem }]">
          <img v-if="m.ref" class="ref" :src="m.ref" alt="Reference picture" />
          <span><strong>{{ m.who === 'member' ? 'You' : who }}</strong> {{ m.text }}</span>
        </div>
        <div v-if="paint.busy" class="line companion typing">{{ who }} is drawing…</div>
      </div>
      <div class="chips">
        <button v-for="s in STYLES" :key="s" class="chip" :class="{ on: paint.style === s }" :aria-pressed="paint.style === s" @click="paint.style = paint.style === s ? '' : s">{{ s }}</button>
        <span class="spacer"></span>
        <template v-if="paint.reference">
          <img class="ref-chip" :src="paint.reference.url" :alt="paint.reference.name" :title="paint.reference.name" />
          <button class="chip" :class="{ on: paint.trace }" :aria-pressed="paint.trace" title="Show the picture at 30% under your strokes (never saved)" @click="paint.trace = !paint.trace">Trace</button>
          <button class="chip" title="Take the picture away" aria-label="Take the picture away" @click="dropReference">✕</button>
        </template>
        <button v-else class="chip" title="Pick, paste or drop a picture to follow" @click="picker.click()">＋ Picture</button>
        <input ref="picker" type="file" accept="image/png,image/jpeg,image/webp" hidden @change="onPick" />
      </div>
      <div class="compose">
        <textarea v-model="draft" rows="2" maxlength="1000" :placeholder="`Ask ${who} to draw… (paste a picture here too)`" aria-label="What to draw" @paste="onPaste" @keydown.enter.exact.prevent="send"></textarea>
        <button class="primary" :disabled="!canSend" @click="send">{{ paint.busy ? 'Drawing…' : 'Draw' }}</button>
      </div>
    </section>
  </div>
</template>

<style scoped>
.paint {
  display: grid;
  flex: 1;
  grid-template-columns: 196px minmax(0, 1fr) 250px;
  grid-template-rows: minmax(0, 1fr) 220px;
  gap: 16px;
  min-height: 0;
}
.widget {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
  padding: 14px;
  background: var(--panel-background);
  border: 1px solid var(--line);
  border-radius: 14px;
  -webkit-backdrop-filter: blur(var(--glass-blur));
  backdrop-filter: blur(var(--glass-blur));
}
h3 {
  margin: 0;
  font-size: 13px;
  font-weight: 500;
  letter-spacing: 0.18em;
}

/* the tools and the colour wheel */
.tools {
  grid-row: 1 / 3;
  overflow-y: auto;
  scrollbar-width: var(--scrollbar-width);
  scrollbar-color: var(--scrollbar-thumb) var(--scrollbar-track);
}
.tool-grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 6px;
}
.tool-grid button {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 2px;
  padding: 8px 4px;
  font-size: 12px;
}
.tool-grid .icon {
  font-size: 18px;
  line-height: 1;
}
.tool-grid button.on,
.chip.on,
.bar .active {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 85%, transparent);
  color: #07142e;
}
.slider {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 12px;
  color: var(--text-dim);
}
.slider input {
  width: 100%;
}
.wheel-box {
  position: relative;
  width: 150px;
  height: 150px;
  align-self: center;
}
.wheel {
  display: block;
  width: 150px;
  height: 150px;
  border-radius: 50%;
  cursor: crosshair;
  touch-action: none;
}
.marker {
  position: absolute;
  width: 12px;
  height: 12px;
  margin: -7px 0 0 -7px;
  border: 2px solid #fff;
  border-radius: 50%;
  box-shadow: 0 0 0 1px #000;
  pointer-events: none;
}
.swatches {
  display: flex;
  align-items: center;
  gap: 8px;
}
.swatch {
  width: 34px;
  height: 34px;
  padding: 0;
  border: 1px solid var(--line);
  border-radius: 8px;
}
.swatch.last {
  width: 24px;
  height: 24px;
}
.swatch-words {
  font-size: 11px;
  color: var(--text-dim);
}

/* the canvas */
.canvas-box {
  grid-column: 2;
  grid-row: 1;
}
.bar {
  position: relative;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}
.bar .name {
  width: 190px;
  padding: 6px 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}
.dirty {
  color: var(--warn);
  font-size: 12px;
}
.vector {
  font-size: 12px;
}
.confirm {
  display: flex;
  align-items: center;
  gap: 8px;
  font-size: 13px;
}
.open-strip {
  display: flex;
  gap: 10px;
  overflow-x: auto;
  flex-shrink: 0;
  scrollbar-width: var(--scrollbar-width);
  scrollbar-color: var(--scrollbar-thumb) var(--scrollbar-track);
}
.open-thumb {
  display: flex;
  flex-direction: column;
  gap: 4px;
  width: 120px;
  flex-shrink: 0;
  padding: 4px;
  font-size: 11px;
}
.open-thumb img {
  width: 100%;
  height: 72px;
  object-fit: cover;
  border-radius: 6px;
  background: #fff;
}
.open-thumb span {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.wrap {
  display: flex;
  flex: 1;
  align-items: center;
  justify-content: center;
  min-height: 0;
  overflow: hidden;
}
.stage {
  position: relative;
  overflow: hidden;
  border-radius: 6px;
  box-shadow: 0 4px 24px rgba(0, 0, 0, 0.35);
  background-color: #fff;
  background-image: linear-gradient(45deg, #e6e6e6 25%, transparent 25%, transparent 75%, #e6e6e6 75%), linear-gradient(45deg, #e6e6e6 25%, transparent 25%, transparent 75%, #e6e6e6 75%);
  background-position: 0 0, 8px 8px;
  background-size: 16px 16px;
}
.layer {
  position: absolute;
  inset: 0;
  pointer-events: none;
}
.layer.svg :deep(svg) {
  display: block;
  width: 100%;
  height: 100%;
}
.trace {
  width: 100%;
  height: 100%;
  object-fit: contain;
  opacity: 0.3;
}
.input {
  position: absolute;
  inset: 0;
  touch-action: none;
  cursor: crosshair;
}
.input.bucket {
  cursor: cell;
}
.input.blocked {
  cursor: not-allowed;
}

/* LAYERS */
.layers {
  grid-column: 3;
  grid-row: 1;
}
.layers header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.layers ul {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin: 0;
  padding: 0;
  overflow-y: auto;
  list-style: none;
  scrollbar-width: var(--scrollbar-width);
  scrollbar-color: var(--scrollbar-thumb) var(--scrollbar-track);
}
.layers li {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 4px 6px;
  border: 1px solid transparent;
  border-radius: 8px;
  cursor: pointer;
}
.layers li:hover {
  background: color-mix(in srgb, var(--accent) 10%, transparent);
}
.layers li.active {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 18%, transparent);
}
.layers li.companion .layer-name {
  font-style: italic;
}
.eye {
  width: 24px;
  padding: 2px 0;
  border: 0;
  font-size: 14px;
}
.thumb {
  width: 48px;
  height: 30px;
  flex-shrink: 0;
  object-fit: contain;
  border: 1px solid var(--line);
  border-radius: 4px;
  background-color: #fff;
  background-image: linear-gradient(45deg, #ddd 25%, transparent 25%, transparent 75%, #ddd 75%), linear-gradient(45deg, #ddd 25%, transparent 25%, transparent 75%, #ddd 75%);
  background-position: 0 0, 4px 4px;
  background-size: 8px 8px;
}
.layer-name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  font-size: 13px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.rename {
  flex: 1;
  min-width: 0;
  padding: 2px 6px;
  border: 1px solid var(--accent);
  border-radius: 6px;
  background: rgba(0, 0, 0, 0.3);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}
.row-actions {
  display: flex;
  gap: 2px;
  opacity: 0.55;
}
.layers li:hover .row-actions,
.layers li.active .row-actions {
  opacity: 1;
}
.row-actions button {
  padding: 2px 5px;
  font-size: 11px;
}

/* the chat */
.chat {
  grid-column: 2 / 4;
  grid-row: 2;
}
.chat header {
  display: flex;
  align-items: center;
  gap: 10px;
}
.lines {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 6px;
  min-height: 0;
  overflow-y: auto;
  font-size: 13px;
  scrollbar-width: var(--scrollbar-width);
  scrollbar-color: var(--scrollbar-thumb) var(--scrollbar-track);
}
.line {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  max-width: 85%;
  padding: 6px 10px;
  border-radius: 10px;
  background: color-mix(in srgb, var(--panel-background) 60%, transparent);
}
.line.member {
  align-self: flex-end;
  background: color-mix(in srgb, var(--accent) 22%, transparent);
}
.line.problem {
  color: var(--warn);
}
.line.typing {
  color: var(--text-dim);
  font-style: italic;
}
.line .ref {
  width: 48px;
  height: 48px;
  object-fit: cover;
  border-radius: 6px;
}
.chips {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
}
.chip {
  padding: 4px 12px;
  border-radius: 999px;
  font-size: 12px;
}
.ref-chip {
  width: 30px;
  height: 30px;
  object-fit: cover;
  border: 1px solid var(--line);
  border-radius: 6px;
}
.compose {
  display: flex;
  gap: 10px;
}
.compose textarea {
  flex: 1;
  min-width: 0;
  padding: 8px 10px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 13px;
  resize: none;
}
.compose .primary {
  min-width: 96px;
}

.spacer {
  flex: 1;
}
.problem {
  margin: 0;
  font-size: 13px;
  color: var(--warn);
}
.hint,
.empty {
  margin: 0;
  font-size: 13px;
  color: var(--text-dim);
}
button {
  padding: 6px 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}
button.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: #07142e;
  font-weight: 600;
}
button.danger {
  border-color: color-mix(in srgb, #ff6b81 60%, var(--line));
  color: #ff9aa9;
}
button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
</style>
