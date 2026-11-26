// PHOTO's drawing: canvases for the layers, the composite (blends, opacity, text, then ADJUST), the
// transforms of a layer's pixels, the selection as a clip path and as marching ants, and the brush's
// dabs. Every edit makes a new canvas (a pill may keep the old one), never changes one in place.
import { adjustPixels, isNeutral } from './adjust.js'
import { compositeOf } from './doc.js'
import { maskOutline, maskRuns } from './select.js'

export function makeCanvas(w, h) {
  const c = document.createElement('canvas')
  c.width = Math.max(1, Math.round(w))
  c.height = Math.max(1, Math.round(h))
  return c
}

export function copyCanvas(src) {
  const c = makeCanvas(src.width, src.height)
  c.getContext('2d').drawImage(src, 0, 0)
  return c
}

/** A picture (a data URL) drawn onto a new canvas. */
export function loadCanvas(url) {
  return new Promise((resolve, reject) => {
    const img = new Image()
    img.onload = () => {
      const c = makeCanvas(img.naturalWidth, img.naturalHeight)
      c.getContext('2d').drawImage(img, 0, 0)
      resolve(c)
    }
    img.onerror = () => reject(new Error('The picture could not be opened.'))
    img.src = url
  })
}

/** A layer's pixels turned by quarter turns clockwise. */
export function rotateCanvas(src, turns) {
  const t = ((turns % 4) + 4) % 4
  if (!t) return copyCanvas(src)
  const odd = t % 2 === 1
  const c = makeCanvas(odd ? src.height : src.width, odd ? src.width : src.height)
  const ctx = c.getContext('2d')
  ctx.translate(c.width / 2, c.height / 2)
  ctx.rotate((t * Math.PI) / 2)
  ctx.drawImage(src, -src.width / 2, -src.height / 2)
  return c
}

export function flipCanvas(src, axis) {
  const c = makeCanvas(src.width, src.height)
  const ctx = c.getContext('2d')
  if (axis === 'h') {
    ctx.translate(c.width, 0)
    ctx.scale(-1, 1)
  } else {
    ctx.translate(0, c.height)
    ctx.scale(1, -1)
  }
  ctx.drawImage(src, 0, 0)
  return c
}

export function scaleCanvas(src, sx, sy) {
  const c = makeCanvas(src.width * sx, src.height * sy)
  const ctx = c.getContext('2d')
  ctx.imageSmoothingQuality = 'high'
  ctx.drawImage(src, 0, 0, c.width, c.height)
  return c
}

export function fontOf(text) {
  return `${text.size}px ${text.font || 'system-ui'}, sans-serif`
}

/** A text layer's lines and box, measured on `ctx`. */
export function textBox(ctx, layer) {
  ctx.save()
  ctx.font = fontOf(layer.text)
  const lines = String(layer.text.value ?? '').split('\n')
  const w = Math.max(1, ...lines.map((l) => ctx.measureText(l).width))
  ctx.restore()
  return { lines, w: Math.ceil(w), h: Math.ceil(lines.length * layer.text.size * 1.2) }
}

function drawText(ctx, layer) {
  const { lines } = textBox(ctx, layer)
  ctx.font = fontOf(layer.text)
  ctx.fillStyle = layer.text.color
  ctx.textBaseline = 'top'
  lines.forEach((line, i) => ctx.fillText(line, layer.x, layer.y + i * layer.text.size * 1.2))
}

/** The layers drawn onto `ctx` (a W x H canvas) in order, each with its opacity and blend, then ADJUST.
 * With `rect` only that part is drawn again (the brush), through a small canvas of its own padded a
 * little so the sharpness sees its neighbours. */
export function renderComposite(ctx, doc, pixels, { rect = null, adjust = true, background = null } = {}) {
  const W = doc.width
  const H = doc.height
  const inner = { x: 0, y: 0, w: W, h: H }
  if (rect) {
    inner.x = Math.max(0, Math.floor(rect.x))
    inner.y = Math.max(0, Math.floor(rect.y))
    inner.w = Math.min(W, Math.ceil(rect.x + rect.w)) - inner.x
    inner.h = Math.min(H, Math.ceil(rect.y + rect.h)) - inner.y
    if (inner.w <= 0 || inner.h <= 0) return
  }
  const adjusting = adjust && !isNeutral(doc.adjust)
  const pad = rect && adjusting && doc.adjust.sharpness ? 2 : 0
  const r = { x: Math.max(0, inner.x - pad), y: Math.max(0, inner.y - pad) }
  r.w = Math.min(W, inner.x + inner.w + pad) - r.x
  r.h = Math.min(H, inner.y + inner.h + pad) - r.y
  const scratch = rect ? makeCanvas(r.w, r.h) : null
  const g = scratch ? scratch.getContext('2d', { willReadFrequently: true }) : ctx
  g.save()
  if (scratch) g.translate(-r.x, -r.y)
  else g.clearRect(0, 0, W, H)
  if (background) {
    g.fillStyle = background
    g.fillRect(r.x, r.y, r.w, r.h)
  }
  for (const layer of doc.layers) {
    if (!layer.visible || layer.opacity <= 0) continue
    g.globalAlpha = layer.opacity / 100
    g.globalCompositeOperation = compositeOf(layer.blend)
    if (layer.kind === 'text') {
      if (layer.text?.value) drawText(g, layer)
    } else if (pixels[layer.id]) {
      g.drawImage(pixels[layer.id], layer.x, layer.y)
    }
  }
  g.restore()
  if (adjusting) {
    const img = g.getImageData(0, 0, r.w, r.h)
    adjustPixels(img.data, r.w, r.h, doc.adjust)
    if (scratch) ctx.putImageData(img, r.x, r.y, inner.x - r.x, inner.y - r.y, inner.w, inner.h)
    else ctx.putImageData(img, 0, 0)
  } else if (scratch) {
    ctx.clearRect(inner.x, inner.y, inner.w, inner.h)
    ctx.drawImage(scratch, inner.x - r.x, inner.y - r.y, inner.w, inner.h, inner.x, inner.y, inner.w, inner.h)
  }
}

/** The whole picture flattened onto a new canvas, ADJUST applied (as exported). */
export function flatten(doc, pixels, { background = null, adjust = true } = {}) {
  const c = makeCanvas(doc.width, doc.height)
  renderComposite(c.getContext('2d', { willReadFrequently: true }), doc, pixels, { adjust, background })
  return c
}

/** A canvas scaled down so its longest side is at most `max` (the chat's copy, the thumbnail). */
export function shrink(src, max) {
  const k = Math.min(1, max / Math.max(src.width, src.height))
  return k >= 1 ? src : scaleCanvas(src, k, k)
}

/** The selection as a clip path, in picture coordinates (cached on the selection). */
export function selectionPath(sel) {
  if (!sel) return null
  if (sel.path) return sel.path
  const p = new Path2D()
  if (sel.kind === 'rect') p.rect(sel.x, sel.y, sel.w, sel.h)
  else if (sel.kind === 'lasso') {
    sel.points.forEach(([x, y], i) => (i ? p.lineTo(x, y) : p.moveTo(x, y)))
    p.closePath()
  } else if (sel.kind === 'mask') {
    for (const [x, y, w, h] of maskRuns(sel.mask, sel.w, sel.h)) p.rect(x, y, w, h)
  }
  Object.defineProperty(sel, 'path', { value: p, enumerable: false })
  return p
}

/** The selection's edges for the marching ants (cached on the selection). */
export function antsPath(sel) {
  if (!sel) return null
  if (sel.ants) return sel.ants
  let p
  if (sel.kind === 'mask') {
    p = new Path2D()
    for (const [x1, y1, x2, y2] of maskOutline(sel.mask, sel.w, sel.h)) {
      p.moveTo(x1, y1)
      p.lineTo(x2, y2)
    }
  } else {
    p = selectionPath(sel)
  }
  Object.defineProperty(sel, 'ants', { value: p, enumerable: false })
  return p
}

/** One soft round dab of `color` (hardness 0..100) on `ctx` at (x, y). */
export function dab(ctx, x, y, size, hardness, color) {
  const r = Math.max(0.5, size / 2)
  if (hardness >= 100) {
    ctx.fillStyle = color
    ctx.beginPath()
    ctx.arc(x, y, r, 0, Math.PI * 2)
    ctx.fill()
    return
  }
  const g = ctx.createRadialGradient(x, y, 0, x, y, r)
  const solid = Math.min(0.99, hardness / 100)
  g.addColorStop(0, color)
  g.addColorStop(solid, color)
  g.addColorStop(1, transparent(color))
  ctx.fillStyle = g
  ctx.beginPath()
  ctx.arc(x, y, r, 0, Math.PI * 2)
  ctx.fill()
}

function transparent(hex) {
  const n = parseInt(hex.slice(1), 16)
  return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, 0)`
}

/** A clone stamp dab: `source` (the layer as the stroke began) shifted by (dx, dy), in a soft circle. */
export function cloneDab(ctx, source, x, y, dx, dy, size, hardness) {
  const r = Math.max(1, size / 2)
  const d = Math.ceil(r * 2)
  const tmp = makeCanvas(d, d)
  const t = tmp.getContext('2d')
  t.drawImage(source, x + dx - r, y + dy - r, d, d, 0, 0, d, d)
  t.globalCompositeOperation = 'destination-in'
  dab(t, r, r, size, hardness, '#000000')
  ctx.drawImage(tmp, x - r, y - r)
}

/** The points between two pointer positions where dabs go, a quarter of the brush apart. */
export function dabPoints(a, b, size) {
  const step = Math.max(1, size / 4)
  const dist = Math.hypot(b.x - a.x, b.y - a.y)
  const n = Math.max(1, Math.ceil(dist / step))
  const out = []
  for (let i = 1; i <= n; i++) out.push({ x: a.x + ((b.x - a.x) * i) / n, y: a.y + ((b.y - a.y) * i) / n })
  return out
}

/** A canvas as a PNG data URL. */
export function pngOf(canvas) {
  return canvas.toDataURL('image/png')
}

/** A canvas encoded as JPEG (quality 0..1) on a white ground, as a data URL. */
export function jpegOf(canvas, quality) {
  return new Promise((resolve, reject) => {
    const c = makeCanvas(canvas.width, canvas.height)
    const ctx = c.getContext('2d')
    ctx.fillStyle = '#ffffff'
    ctx.fillRect(0, 0, c.width, c.height)
    ctx.drawImage(canvas, 0, 0)
    c.toBlob(
      (blob) => {
        if (!blob) return reject(new Error('The JPG could not be made.'))
        const reader = new FileReader()
        reader.onload = () => resolve(reader.result)
        reader.onerror = () => reject(new Error('The JPG could not be made.'))
        reader.readAsDataURL(blob)
      },
      'image/jpeg',
      Math.min(1, Math.max(0.1, quality)),
    )
  })
}
