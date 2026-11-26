// Rust thinks, the canvas paints. This walks one display list (scriptorium/src/ground/display.rs,
// DisplayList::to_json) onto a 2D context. Glyphs are filled from outlines fetched from the Rust side
// (Resources::glyphs_json) - no browser font ever decides a glyph's shape - and images come as RGBA
// pixels (Resources::image_json).
//
// Pure where it can be: the context, the Path2D maker and the image decoder are all handed in, so the
// tests run in node with a recording fake context.

import { IDENTITY, applyRect, multiply, toRect } from './tiles.js'

/** `new Path2D(d)` - the default way to turn SVG path data into something `ctx.fill` takes. */
export function defaultMakePath(d) {
  return new Path2D(d)
}

// ---------------------------------------------------------------------------------------------------
// Base64 (the image pixels), by hand
// ---------------------------------------------------------------------------------------------------

const B64 = (() => {
  const t = new Int16Array(128).fill(-1)
  const abc = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'
  for (let i = 0; i < 64; i++) t[abc.charCodeAt(i)] = i
  // the URL-safe alphabet too
  t[45] = 62
  t[95] = 63
  return t
})()

/** Standard base64 -> bytes (whitespace and padding ignored). */
export function decodeBase64(s) {
  const str = String(s || '')
  const out = new Uint8ClampedArray(Math.floor((str.length * 3) / 4))
  let o = 0, buf = 0, bits = 0
  for (let i = 0; i < str.length; i++) {
    const c = str.charCodeAt(i)
    const v = c < 128 ? B64[c] : -1
    if (v < 0) continue
    buf = ((buf << 6) | v) & 0xffffff
    bits += 6
    if (bits >= 8) {
      bits -= 8
      out[o++] = (buf >> bits) & 255
    }
  }
  return o === out.length ? out : out.subarray(0, o)
}

function parse(json) {
  return typeof json === 'string' ? JSON.parse(json) : json
}

// ---------------------------------------------------------------------------------------------------
// Glyph outlines
// ---------------------------------------------------------------------------------------------------

/**
 * Font id -> units per em and gid -> outline. Outlines arrive as SVG path data in font units, y up
 * (glyphs_json); the Path2D is built the first time a glyph is painted. An empty outline (a space) is
 * kept as "loaded, nothing to fill".
 */
export class GlyphCache {
  constructor({ loadGlyphs = null, makePath = defaultMakePath } = {}) {
    this.loadGlyphs = loadGlyphs
    this.makePath = makePath
    this.fonts = new Map()
    this.pending = new Map() // "font:gid" -> promise
  }

  font(id) {
    const k = Number(id)
    let f = this.fonts.get(k)
    if (!f) {
      f = { upem: 1000, outlines: new Map(), paths: new Map(), unavailable: new Set() }
      this.fonts.set(k, f)
    }
    return f
  }

  /** Units per em of a font (1000 until the Rust side says otherwise). */
  upem(id) {
    return this.fonts.get(Number(id))?.upem ?? 1000
  }

  /** Store one glyphs_json answer (string or object). Returns how many outlines it carried. */
  add(json) {
    const j = parse(json)
    if (!j || j.font === undefined || j.font === null) return 0
    const f = this.font(j.font)
    if (+j.upem > 0) f.upem = +j.upem
    let n = 0
    for (const [k, d] of Object.entries(j.glyphs || {})) {
      const gid = Number(k)
      f.outlines.set(gid, typeof d === 'string' ? d : '')
      f.paths.delete(gid)
      f.unavailable.delete(gid)
      n++
    }
    for (const gid of j.missing || []) if (!f.outlines.has(Number(gid))) f.unavailable.add(Number(gid))
    return n
  }

  /** Put one outline in by hand. */
  setGlyph(font, gid, d, upem) {
    const f = this.font(font)
    if (+upem > 0) f.upem = +upem
    f.outlines.set(Number(gid), d || '')
    f.paths.delete(Number(gid))
  }

  has(font, gid) {
    return !!this.fonts.get(Number(font))?.outlines.has(Number(gid))
  }

  /** The Path2D (built now if needed), null for a glyph with nothing to fill, undefined when not loaded. */
  path(font, gid) {
    const f = this.fonts.get(Number(font))
    if (!f) return undefined
    const g = Number(gid)
    if (f.paths.has(g)) return f.paths.get(g)
    const d = f.outlines.get(g)
    if (d === undefined) return undefined
    const p = d.trim() ? this.makePath(d) : null
    f.paths.set(g, p)
    return p
  }

  /** The gids (unique, in first-seen order) whose outlines are not loaded yet. */
  missing(font, gids) {
    const f = this.fonts.get(Number(font))
    const seen = new Set()
    const out = []
    for (const raw of gids) {
      const g = Number(raw)
      if (seen.has(g)) continue
      seen.add(g)
      if (!f || !f.outlines.has(g)) out.push(g)
    }
    return out
  }

  /** Gids the Rust side said it does not have yet (its `missing` answer). */
  unavailable(font) {
    return [...(this.fonts.get(Number(font))?.unavailable || [])]
  }

  /**
   * Load what is missing through `loadGlyphs(font, gids)` (which answers a glyphs_json string or
   * object). Requests already on their way are shared. Resolves to the gids still missing afterwards.
   */
  async ensure(font, gids) {
    const want = this.missing(font, gids)
    if (!want.length || !this.loadGlyphs) return want
    const waits = new Set()
    const ask = []
    for (const g of want) {
      const p = this.pending.get(`${font}:${g}`)
      if (p) waits.add(p)
      else ask.push(g)
    }
    if (ask.length) {
      const p = Promise.resolve()
        .then(() => this.loadGlyphs(Number(font), ask))
        .then((json) => {
          if (json) this.add(json)
        })
        .finally(() => {
          for (const g of ask) if (this.pending.get(`${font}:${g}`) === p) this.pending.delete(`${font}:${g}`)
        })
      for (const g of ask) this.pending.set(`${font}:${g}`, p)
      waits.add(p)
    }
    await Promise.all(waits)
    return this.missing(font, want)
  }

  clear() {
    this.fonts.clear()
    this.pending.clear()
  }
}

// ---------------------------------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------------------------------

function makeCanvas(w, h) {
  if (typeof OffscreenCanvas !== 'undefined') return new OffscreenCanvas(w, h)
  if (typeof document !== 'undefined') {
    const c = document.createElement('canvas')
    c.width = w
    c.height = h
    return c
  }
  return null
}

/**
 * image_json -> something `drawImage` takes: an ImageBitmap where the webview has createImageBitmap,
 * else a canvas holding the pixels. Null for an empty image.
 */
export async function defaultDecodeImage(json) {
  const w = json.width | 0, h = json.height | 0
  if (w <= 0 || h <= 0) return null
  let px = decodeBase64(json.rgba)
  if (px.length !== w * h * 4) {
    const fixed = new Uint8ClampedArray(w * h * 4)
    fixed.set(px.subarray(0, fixed.length))
    px = fixed
  }
  const data = new ImageData(px, w, h)
  if (typeof createImageBitmap === 'function') return createImageBitmap(data)
  const c = makeCanvas(w, h)
  c.getContext('2d').putImageData(data, 0, 0)
  return c
}

/** Image id -> drawable, loaded through `loadImage(id)` (which answers an image_json string or object). */
export class ImageCache {
  constructor({ loadImage = null, decode = defaultDecodeImage } = {}) {
    this.loadImage = loadImage
    this.decode = decode
    this.images = new Map() // id -> drawable or null (empty image)
    this.sizes = new Map() // id -> { width, height }
    this.pending = new Map()
  }

  /** Store one image_json answer; resolves to the drawable. */
  async add(json) {
    const j = parse(json)
    if (!j || j.image === undefined || j.image === null) return null
    const id = Number(j.image)
    const drawable = await this.decode(j)
    this.set(id, drawable, { width: j.width | 0, height: j.height | 0 })
    return drawable
  }

  set(id, drawable, size = null) {
    const k = Number(id)
    const old = this.images.get(k)
    if (old && old !== drawable && typeof old.close === 'function') old.close()
    this.images.set(k, drawable)
    if (size) this.sizes.set(k, size)
  }

  has(id) {
    return this.images.has(Number(id))
  }

  /** The drawable, null for an empty image, undefined when not loaded. */
  get(id) {
    return this.images.get(Number(id))
  }

  missing(ids) {
    const out = []
    const seen = new Set()
    for (const raw of ids) {
      const k = Number(raw)
      if (seen.has(k)) continue
      seen.add(k)
      if (!this.images.has(k)) out.push(k)
    }
    return out
  }

  /** Load every missing image (one `loadImage` call each, shared while in flight). Resolves to those still missing. */
  async ensure(ids) {
    const want = this.missing(ids)
    if (!want.length || !this.loadImage) return want
    await Promise.all(
      want.map((id) => {
        let p = this.pending.get(id)
        if (!p) {
          p = Promise.resolve()
            .then(() => this.loadImage(id))
            .then((json) => (json ? this.add(json) : null))
            .finally(() => this.pending.delete(id))
          this.pending.set(id, p)
        }
        return p
      }),
    )
    return this.missing(want)
  }

  clear() {
    for (const v of this.images.values()) if (v && typeof v.close === 'function') v.close()
    this.images.clear()
    this.sizes.clear()
    this.pending.clear()
  }
}

// ---------------------------------------------------------------------------------------------------
// What a list needs
// ---------------------------------------------------------------------------------------------------

/** `{glyphs: Map<font, Set<gid>>, images: Set<id>}` - everything to fetch before the list paints whole. */
export function collectNeeds(list) {
  const l = parse(list)
  const glyphs = new Map()
  const images = new Set()
  for (const it of l?.items || []) {
    if (it.t === 'glyphs') {
      const font = Number(it.font)
      let s = glyphs.get(font)
      if (!s) glyphs.set(font, (s = new Set()))
      const g = it.g || []
      for (let i = 0; i + 2 < g.length; i += 3) s.add(Number(g[i]))
    } else if (it.t === 'image') {
      images.add(Number(it.image))
    }
  }
  return { glyphs, images }
}

/** Every link with its box in page coordinates (pushed transforms applied), in list order. */
export function collectLinks(list) {
  const l = parse(list)
  const out = []
  let m = IDENTITY
  const stack = []
  for (const it of l?.items || []) {
    if (it.t === 'transform') {
      stack.push(m)
      m = multiply(m, it.m || IDENTITY)
    } else if (it.t === 'untransform') {
      m = stack.pop() || IDENTITY
    } else if (it.t === 'link') {
      const r = toRect(it.r)
      if (r) out.push({ target: it.target, rect: applyRect(m, r) })
    }
  }
  return out
}

/** The topmost link under a page point, or null. */
export function hitLink(links, x, y) {
  for (let i = links.length - 1; i >= 0; i--) {
    const r = links[i].rect
    if (x >= r.x && x <= r.x + r.w && y >= r.y && y <= r.y + r.h) return links[i]
  }
  return null
}

// ---------------------------------------------------------------------------------------------------
// Painting
// ---------------------------------------------------------------------------------------------------

function fillStyleFor(ctx, p) {
  if (!p) return null
  switch (p.k) {
    case 'solid':
      return p.c
    case 'linear': {
      const g = ctx.createLinearGradient(+p.x0, +p.y0, +p.x1, +p.y1)
      for (const [off, c] of p.stops || []) g.addColorStop(Math.min(1, Math.max(0, +off)), c)
      return g
    }
    case 'radial': {
      const g = ctx.createRadialGradient(+p.fx, +p.fy, Math.max(0, +p.fr), +p.cx, +p.cy, Math.max(0, +p.r))
      for (const [off, c] of p.stops || []) g.addColorStop(Math.min(1, Math.max(0, +off)), c)
      return g
    }
    default:
      return null
  }
}

/**
 * Paint `list` onto `ctx`. Options:
 * - `glyphs` (GlyphCache), `images` (ImageCache): the resources; what they lack is reported, not painted
 * - `scale`: device pixels per point (zoom x dpr), default 1
 * - `viewport` ({x, y, w, h} page points): the part of the page that lands at the context's (0, 0)
 * - `indices`: paint only these items (from visibleItems; state items must be among them)
 * - `background`: page colour filled first (default `list.bg` or white; false for none)
 * - `makePath`: SVG path data -> Path2D (default `new Path2D(d)`)
 * - `pathCache`: a WeakMap item -> Path2D kept between paints of the same list
 *
 * The context's transform on entry is kept (a host can pre-apply its own); every save is matched by a
 * restore before returning, even for an unbalanced list. Returns `{links, missing: {glyphs, images},
 * painted}` with link boxes in page coordinates.
 */
export function paintList(ctx, list, opts = {}) {
  const l = parse(list)
  const {
    glyphs = null,
    images = null,
    scale = 1,
    viewport = null,
    indices = null,
    makePath = defaultMakePath,
    pathCache = null,
  } = opts
  const background = opts.background === undefined ? l?.bg || '#ffffff' : opts.background
  const links = []
  const missing = { glyphs: new Map(), images: new Set() }
  let painted = 0
  if (!l) return { links, missing, painted }
  const items = l.items || []
  const vp = toRect(viewport)
  const ox = vp ? vp.x : 0, oy = vp ? vp.y : 0

  ctx.save()
  ctx.transform(scale, 0, 0, scale, -ox * scale, -oy * scale)
  if ('imageSmoothingQuality' in ctx) ctx.imageSmoothingQuality = 'high'
  if (background) {
    ctx.fillStyle = background
    ctx.fillRect(0, 0, +l.w || 0, +l.h || 0)
  }
  const hairline = 1 / (scale || 1)

  const pathOf = (it) => {
    if (!it.d) return null
    if (pathCache) {
      let p = pathCache.get(it)
      if (p === undefined) pathCache.set(it, (p = makePath(it.d)))
      return p
    }
    return makePath(it.d)
  }

  // the painter's own stack: what each save was for, and the transform at that point (for links)
  const stack = []
  let m = IDENTITY
  const pop = () => {
    if (!stack.length) return // an unmatched pop never digs below our own save
    m = stack.pop().m
    ctx.restore()
  }

  const n = indices ? indices.length : items.length
  for (let k = 0; k < n; k++) {
    const it = indices ? items[indices[k]] : items[k]
    if (!it) continue
    switch (it.t) {
      case 'clip': {
        stack.push({ kind: 'clip', m })
        ctx.save()
        const p = pathOf(it)
        if (p) ctx.clip(p)
        else {
          // a clip to nothing hides everything up to its unclip
          ctx.beginPath()
          ctx.rect(0, 0, 0, 0)
          ctx.clip()
        }
        break
      }
      case 'transform': {
        const t = it.m || IDENTITY
        stack.push({ kind: 'transform', m })
        ctx.save()
        ctx.transform(+t[0], +t[1], +t[2], +t[3], +t[4], +t[5])
        m = multiply(m, t)
        break
      }
      case 'opacity': {
        stack.push({ kind: 'opacity', m })
        ctx.save()
        ctx.globalAlpha = ctx.globalAlpha * Math.min(1, Math.max(0, +it.a))
        break
      }
      case 'unclip':
      case 'untransform':
      case 'unopacity':
        pop()
        break
      case 'path': {
        const p = pathOf(it)
        if (!p) break
        if (it.fill) {
          const style = fillStyleFor(ctx, it.fill)
          if (style) {
            ctx.fillStyle = style
            ctx.fill(p)
          }
        }
        const s = it.stroke
        if (s) {
          ctx.lineWidth = +s.w > 0 ? +s.w : hairline
          ctx.strokeStyle = s.c
          ctx.setLineDash(Array.isArray(s.dash) ? s.dash.map(Number) : [])
          ctx.lineDashOffset = +s.off || 0
          ctx.lineCap = s.cap || 'butt'
          ctx.lineJoin = s.join || 'miter'
          ctx.miterLimit = +s.ml > 0 ? +s.ml : 10
          ctx.stroke(p)
        }
        painted++
        break
      }
      case 'glyphs': {
        const font = Number(it.font)
        const size = +it.size || 0
        const upem = glyphs ? glyphs.upem(font) : 1000
        const k1 = size / upem
        const g = it.g || []
        ctx.fillStyle = it.c || '#000000'
        for (let i = 0; i + 2 < g.length; i += 3) {
          const gid = Number(g[i])
          const path = glyphs ? glyphs.path(font, gid) : undefined
          if (path === undefined) {
            let s = missing.glyphs.get(font)
            if (!s) missing.glyphs.set(font, (s = new Set()))
            s.add(gid)
            continue
          }
          if (path === null) continue // nothing to fill (a space)
          ctx.save()
          ctx.translate(+g[i + 1], +g[i + 2])
          ctx.scale(k1, -k1) // outlines are y up, the page is y down
          ctx.fill(path)
          ctx.restore()
        }
        painted++
        break
      }
      case 'image': {
        const r = toRect(it.r)
        const id = Number(it.image)
        const img = images ? images.get(id) : undefined
        if (img === undefined) missing.images.add(id)
        else if (img && r && r.w && r.h) ctx.drawImage(img, r.x, r.y, r.w, r.h)
        painted++
        break
      }
      case 'link': {
        const r = toRect(it.r)
        if (r) links.push({ target: it.target, rect: applyRect(m, r) })
        break
      }
      default:
        break
    }
  }
  while (stack.length) pop()
  ctx.restore()
  return { links, missing, painted }
}
