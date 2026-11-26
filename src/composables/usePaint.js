// STUDIO > PAINT's painting, kept for the whole run (module state), so strokes survive the room or
// the page being hidden and shown again. The member's layers keep their pixels on canvases held
// here (the room only shows them); the companion's layers keep their cleaned SVG.
import { markRaw, reactive } from 'vue'
import { newId } from '../components/paint/layers.js'
import { cleanSvg, svgDataUrl } from '../components/paint/svgclean.js'

export const CANVAS_W = 1280
export const CANVAS_H = 800
const THUMB_W = 72

export const paint = reactive({
  width: CANVAS_W,
  height: CANVAS_H,
  // bottom first: { id, name, owner: 'member' | 'companion', visible, canvas (member), svg (companion) }
  layers: [],
  active: '',
  tool: 'brush',
  size: 8,
  tolerance: 32,
  colour: '#5b4bff',
  last: '#ffffff',
  name: '',
  dirty: false,
  // the chat: { who: 'member' | 'companion', text, ref (a reference picture's address), problem }
  chat: [],
  style: '',
  // { url, name } of the reference picture, or null; trace shows it at 30% under everything
  reference: null,
  trace: false,
  busy: false,
  // a layer's small picture, by its id
  thumbs: {},
})

export function makeCanvas(width, height) {
  const canvas = document.createElement('canvas')
  canvas.width = width
  canvas.height = height
  return markRaw(canvas)
}

export function memberLayer(name, fill = '') {
  const canvas = makeCanvas(paint.width, paint.height)
  if (fill) {
    const ctx = canvas.getContext('2d')
    ctx.fillStyle = fill
    ctx.fillRect(0, 0, canvas.width, canvas.height)
  }
  return { id: newId(), name, owner: 'member', visible: true, canvas }
}

/** A fresh painting: a white Background and an empty Layer 1 to draw on. */
export function startPainting(width = CANVAS_W, height = CANVAS_H) {
  paint.width = width
  paint.height = height
  const background = memberLayer('Background', '#ffffff')
  const first = memberLayer('Layer 1')
  paint.layers = [background, first]
  paint.active = first.id
  paint.name = ''
  paint.dirty = false
  paint.thumbs = {}
  refreshThumb(background)
  refreshThumb(first)
}

export function ensurePainting() {
  if (!paint.layers.length) startPainting()
}

/** A layer's live thumbnail, made again after each stroke. */
export function refreshThumb(layer) {
  if (!layer) return
  if (layer.owner === 'companion') {
    paint.thumbs[layer.id] = svgDataUrl(layer.svg, paint.width, paint.height)
    return
  }
  const small = document.createElement('canvas')
  small.width = THUMB_W
  small.height = Math.max(1, Math.round((THUMB_W * paint.height) / paint.width))
  small.getContext('2d').drawImage(layer.canvas, 0, 0, small.width, small.height)
  paint.thumbs[layer.id] = small.toDataURL('image/png')
}

function loadImage(src) {
  return new Promise((resolve, reject) => {
    const img = new Image()
    img.onload = () => resolve(img)
    img.onerror = () => reject(new Error('A layer could not be drawn.'))
    img.src = src
  })
}

/** Every visible layer drawn onto one canvas, bottom first (the trace picture never). */
export async function flatten() {
  const out = makeCanvas(paint.width, paint.height)
  const ctx = out.getContext('2d')
  for (const layer of paint.layers) {
    if (!layer.visible) continue
    if (layer.owner === 'member') ctx.drawImage(layer.canvas, 0, 0)
    else ctx.drawImage(await loadImage(svgDataUrl(layer.svg, paint.width, paint.height)), 0, 0, paint.width, paint.height)
  }
  return out
}

/** The painting as paint_save wants it. */
export async function paintingForSave(name) {
  return {
    name,
    width: paint.width,
    height: paint.height,
    layers: paint.layers.map((l) => ({ name: l.name, owner: l.owner, visible: l.visible, png: l.owner === 'member' ? l.canvas.toDataURL('image/png') : '', svg: l.owner === 'companion' ? l.svg : '' })),
    flat: (await flatten()).toDataURL('image/png'),
  }
}

/** A painting from paint_open put on the page: the member's PNGs drawn back, the SVGs cleaned again. */
export async function loadPainting(page) {
  paint.width = page.width
  paint.height = page.height
  const layers = []
  for (const l of page.layers) {
    if (l.owner === 'companion') {
      const id = newId()
      const { svg } = cleanSvg(l.svg, page.width, page.height, `${id}-`)
      if (svg) layers.push({ id, name: l.name, owner: 'companion', visible: l.visible, svg })
    } else {
      const layer = memberLayer(l.name)
      layer.visible = l.visible
      layer.canvas.getContext('2d').drawImage(await loadImage(l.png), 0, 0)
      layers.push(layer)
    }
  }
  paint.layers = layers
  paint.thumbs = {}
  layers.forEach(refreshThumb)
  paint.active = [...layers].reverse().find((l) => l.owner === 'member')?.id ?? ''
  paint.name = page.name
  paint.dirty = false
}
