import { describe, expect, it } from 'vitest'
import { GlyphCache, ImageCache, collectLinks, collectNeeds, decodeBase64, hitLink, paintList } from './painter.js'

// A 2D context that only records what was asked of it.
class FakeCtx {
  constructor() {
    this.calls = []
    this.depth = 0
    this.maxDepth = 0
    this._alpha = 1
    this.alphaStack = []
    this.imageSmoothingQuality = 'low'
  }
  rec(name, ...args) {
    this.calls.push([name, ...args])
  }
  save() {
    this.depth++
    this.maxDepth = Math.max(this.maxDepth, this.depth)
    this.alphaStack.push(this._alpha)
    this.rec('save')
  }
  restore() {
    this.depth--
    this._alpha = this.alphaStack.pop() ?? 1
    this.rec('restore')
  }
  get globalAlpha() {
    return this._alpha
  }
  set globalAlpha(v) {
    this._alpha = v
    this.rec('globalAlpha', v)
  }
  set fillStyle(v) {
    this.rec('fillStyle', v)
  }
  set strokeStyle(v) {
    this.rec('strokeStyle', v)
  }
  set lineWidth(v) {
    this.rec('lineWidth', v)
  }
  set lineDashOffset(v) {
    this.rec('lineDashOffset', v)
  }
  set lineCap(v) {
    this.rec('lineCap', v)
  }
  set lineJoin(v) {
    this.rec('lineJoin', v)
  }
  set miterLimit(v) {
    this.rec('miterLimit', v)
  }
  transform(...a) {
    this.rec('transform', ...a)
  }
  translate(...a) {
    this.rec('translate', ...a)
  }
  scale(...a) {
    this.rec('scale', ...a)
  }
  fill(p) {
    this.rec('fill', p)
  }
  stroke(p) {
    this.rec('stroke', p)
  }
  clip(p) {
    this.rec('clip', p)
  }
  beginPath() {
    this.rec('beginPath')
  }
  rect(...a) {
    this.rec('rect', ...a)
  }
  fillRect(...a) {
    this.rec('fillRect', ...a)
  }
  setLineDash(d) {
    this.rec('setLineDash', d)
  }
  drawImage(...a) {
    this.rec('drawImage', ...a)
  }
  createLinearGradient(...a) {
    const g = { kind: 'linear', args: a, stops: [], addColorStop: (o, c) => g.stops.push([o, c]) }
    this.rec('createLinearGradient', ...a)
    return g
  }
  createRadialGradient(...a) {
    const g = { kind: 'radial', args: a, stops: [], addColorStop: (o, c) => g.stops.push([o, c]) }
    this.rec('createRadialGradient', ...a)
    return g
  }
  names() {
    return this.calls.map((c) => c[0])
  }
  find(name) {
    return this.calls.filter((c) => c[0] === name)
  }
}

const makePath = (d) => ({ d })

function glyphCache(extra = {}) {
  const gc = new GlyphCache({ makePath, ...extra })
  gc.add({ font: 1, upem: 2048, glyphs: { 36: 'M0 0L100 0L100 700Z', 3: '' }, missing: [] })
  return gc
}

describe('paintList state stack', () => {
  it('balances every save with a restore, even for an unbalanced list', () => {
    const ctx = new FakeCtx()
    paintList(ctx, {
      w: 100,
      h: 100,
      items: [
        { t: 'clip', d: 'M0 0L10 0L10 10Z' },
        { t: 'transform', m: [1, 0, 0, 1, 5, 5] },
        { t: 'opacity', a: 0.5 },
        { t: 'path', d: 'M0 0L1 1', fill: { k: 'solid', c: '#ff0000' } },
        { t: 'unopacity' },
        { t: 'untransform' },
        { t: 'unclip' },
        { t: 'unclip' }, // one pop too many
        { t: 'transform', m: [2, 0, 0, 2, 0, 0] }, // never popped
      ],
    }, { makePath })
    expect(ctx.depth).toBe(0)
    expect(ctx.find('save').length).toBe(ctx.find('restore').length)
    expect(ctx.maxDepth).toBe(4) // the painter's own save + clip + transform + opacity
  })

  it('applies the base scale and viewport before the list, transforms in list order', () => {
    const ctx = new FakeCtx()
    paintList(ctx, {
      w: 10,
      h: 10,
      items: [
        { t: 'transform', m: [1, 0, 0, 1, 10, 20] },
        { t: 'transform', m: [2, 0, 0, 3, 0, 0] },
        { t: 'path', d: 'M0 0L1 0', fill: { k: 'solid', c: '#000000' } },
        { t: 'untransform' },
        { t: 'untransform' },
      ],
    }, { makePath, scale: 2, viewport: { x: 100, y: 50, w: 10, h: 10 }, background: false })
    const tr = ctx.find('transform')
    expect(tr[0]).toEqual(['transform', 2, 0, 0, 2, -200, -100])
    expect(tr[1]).toEqual(['transform', 1, 0, 0, 1, 10, 20])
    expect(tr[2]).toEqual(['transform', 2, 0, 0, 3, 0, 0])
    const n = ctx.names()
    expect(n.indexOf('fill')).toBeGreaterThan(n.lastIndexOf('transform'))
    expect(n.slice(-3)).toEqual(['restore', 'restore', 'restore'])
  })

  it('multiplies opacity and restores it', () => {
    const ctx = new FakeCtx()
    paintList(ctx, { w: 1, h: 1, items: [{ t: 'opacity', a: 0.5 }, { t: 'opacity', a: 0.5 }, { t: 'unopacity' }, { t: 'unopacity' }] }, { makePath })
    expect(ctx.find('globalAlpha').map((c) => c[1])).toEqual([0.5, 0.25])
    expect(ctx.globalAlpha).toBe(1)
  })

  it('fills the page white unless the list says otherwise or background is off', () => {
    let ctx = new FakeCtx()
    paintList(ctx, { w: 612, h: 792, items: [] }, { makePath })
    expect(ctx.calls.slice(2, 4)).toEqual([['fillStyle', '#ffffff'], ['fillRect', 0, 0, 612, 792]])
    ctx = new FakeCtx()
    paintList(ctx, { w: 10, h: 10, bg: '#202020', items: [] }, { makePath })
    expect(ctx.find('fillStyle')[0][1]).toBe('#202020')
    ctx = new FakeCtx()
    paintList(ctx, { w: 10, h: 10, items: [] }, { makePath, background: false })
    expect(ctx.find('fillRect').length).toBe(0)
  })

  it('accepts the list as a JSON string', () => {
    const ctx = new FakeCtx()
    const r = paintList(ctx, JSON.stringify({ w: 1, h: 1, items: [{ t: 'path', d: 'M0 0L1 1', fill: { k: 'solid', c: '#123456' } }] }), { makePath })
    expect(r.painted).toBe(1)
    expect(ctx.find('fill')[0][1]).toEqual({ d: 'M0 0L1 1' })
  })

  it('a clip to nothing still clips', () => {
    const ctx = new FakeCtx()
    paintList(ctx, { w: 1, h: 1, items: [{ t: 'clip', d: '' }, { t: 'unclip' }] }, { makePath })
    expect(ctx.find('rect')[0]).toEqual(['rect', 0, 0, 0, 0])
    expect(ctx.find('clip').length).toBe(1)
  })
})

describe('paintList paths', () => {
  it('builds linear and radial gradients with their stops', () => {
    const ctx = new FakeCtx()
    paintList(ctx, {
      w: 10,
      h: 10,
      items: [
        { t: 'path', d: 'M0 0L1 1', fill: { k: 'linear', x0: 0, y0: 0, x1: 10, y1: 0, stops: [[0, '#ff0000'], [1, '#0000ff']] } },
        { t: 'path', d: 'M0 0L1 1', fill: { k: 'radial', fx: 1, fy: 2, fr: 0, cx: 5, cy: 5, r: 9, stops: [[0, '#000000'], [0.5, '#ffffff80'], [1.5, '#00ff00']] } },
      ],
    }, { makePath, background: false })
    expect(ctx.find('createLinearGradient')[0]).toEqual(['createLinearGradient', 0, 0, 10, 0])
    expect(ctx.find('createRadialGradient')[0]).toEqual(['createRadialGradient', 1, 2, 0, 5, 5, 9])
    const styles = ctx.find('fillStyle').map((c) => c[1])
    expect(styles[0].stops).toEqual([[0, '#ff0000'], [1, '#0000ff']])
    expect(styles[1].stops).toEqual([[0, '#000000'], [0.5, '#ffffff80'], [1, '#00ff00']]) // offsets clamped
  })

  it('strokes with width, dash, offset, cap, join and miter limit after the fill', () => {
    const ctx = new FakeCtx()
    paintList(ctx, {
      w: 10,
      h: 10,
      items: [{ t: 'path', d: 'M0 0L5 5', fill: { k: 'solid', c: '#eeeeee' }, stroke: { w: 2, c: '#000000', dash: [3, 1], off: 1.5, cap: 'round', join: 'bevel', ml: 4 } }],
    }, { makePath, background: false })
    const n = ctx.names()
    expect(n.indexOf('fill')).toBeLessThan(n.indexOf('stroke'))
    expect(ctx.find('lineWidth')[0][1]).toBe(2)
    expect(ctx.find('strokeStyle')[0][1]).toBe('#000000')
    expect(ctx.find('setLineDash')[0][1]).toEqual([3, 1])
    expect(ctx.find('lineDashOffset')[0][1]).toBe(1.5)
    expect(ctx.find('lineCap')[0][1]).toBe('round')
    expect(ctx.find('lineJoin')[0][1]).toBe('bevel')
    expect(ctx.find('miterLimit')[0][1]).toBe(4)
  })

  it('a solid stroke clears the dash, and width 0 is a one-device-pixel hairline', () => {
    const ctx = new FakeCtx()
    paintList(ctx, { w: 1, h: 1, items: [{ t: 'path', d: 'M0 0L1 1', stroke: { w: 0, c: '#000000', dash: [], off: 0, cap: 'butt', join: 'miter', ml: 10 } }] }, { makePath, scale: 4 })
    expect(ctx.find('setLineDash')[0][1]).toEqual([])
    expect(ctx.find('lineWidth')[0][1]).toBe(0.25)
    expect(ctx.find('fill').length).toBe(0)
  })

  it('reuses Path2D objects through a path cache', () => {
    let made = 0
    const mk = (d) => (made++, { d })
    const list = { w: 1, h: 1, items: [{ t: 'path', d: 'M0 0L1 1', fill: { k: 'solid', c: '#000000' } }] }
    const pathCache = new WeakMap()
    paintList(new FakeCtx(), list, { makePath: mk, pathCache })
    paintList(new FakeCtx(), list, { makePath: mk, pathCache })
    expect(made).toBe(1)
  })

  it('paints only the indices it is given', () => {
    const ctx = new FakeCtx()
    const items = [
      { t: 'path', d: 'M0 0L1 1', fill: { k: 'solid', c: '#000001' } },
      { t: 'path', d: 'M0 0L1 1', fill: { k: 'solid', c: '#000002' } },
    ]
    const r = paintList(ctx, { w: 1, h: 1, items }, { makePath, indices: [1], background: false })
    expect(r.painted).toBe(1)
    expect(ctx.find('fillStyle').map((c) => c[1])).toEqual(['#000002'])
  })
})

describe('paintList glyphs, images and links', () => {
  it('places each glyph and flips its y-up outline', () => {
    const ctx = new FakeCtx()
    const r = paintList(ctx, { w: 100, h: 100, items: [{ t: 'glyphs', font: 1, size: 12, c: '#333333', g: [36, 10, 50, 3, 20, 50, 36, 30.5, 50] }] }, { makePath, glyphs: glyphCache(), background: false })
    expect(ctx.find('fillStyle')[0][1]).toBe('#333333')
    expect(ctx.find('translate')).toEqual([['translate', 10, 50], ['translate', 30.5, 50]])
    const sc = ctx.find('scale')
    expect(sc.length).toBe(2) // the space (empty outline) is not filled
    expect(sc[0][1]).toBeCloseTo(12 / 2048)
    expect(sc[0][2]).toBeCloseTo(-12 / 2048)
    expect(sc[0][2]).toBeLessThan(0)
    expect(ctx.find('fill')[0][1]).toEqual({ d: 'M0 0L100 0L100 700Z' })
    expect(ctx.depth).toBe(0)
    expect(r.missing.glyphs.size).toBe(0)
  })

  it('reports glyphs it has no outline for instead of guessing a shape', () => {
    const ctx = new FakeCtx()
    const r = paintList(ctx, { w: 1, h: 1, items: [{ t: 'glyphs', font: 1, size: 10, c: '#000000', g: [36, 0, 0, 99, 5, 0] }, { t: 'glyphs', font: 7, size: 10, c: '#000000', g: [1, 0, 0] }] }, { makePath, glyphs: glyphCache(), background: false })
    expect([...r.missing.glyphs.get(1)]).toEqual([99])
    expect([...r.missing.glyphs.get(7)]).toEqual([1])
    expect(ctx.find('fill').length).toBe(1)
  })

  it('draws images into their rect and reports missing ones', () => {
    const ctx = new FakeCtx()
    const images = new ImageCache({ decode: (j) => ({ bitmap: j.image }) })
    images.set(3, { bitmap: 3 })
    const r = paintList(ctx, { w: 100, h: 100, items: [{ t: 'image', image: 3, r: [10, 20, 30, 40] }, { t: 'image', image: 4, r: [0, 0, 1, 1] }] }, { makePath, images, background: false })
    expect(ctx.find('drawImage')).toEqual([['drawImage', { bitmap: 3 }, 10, 20, 30, 40]])
    expect([...r.missing.images]).toEqual([4])
  })

  it('does not paint links but returns them in page coordinates', () => {
    const ctx = new FakeCtx()
    const list = {
      w: 100,
      h: 100,
      items: [
        { t: 'link', r: [1, 2, 3, 4], target: 'https://a' },
        { t: 'transform', m: [2, 0, 0, 2, 10, 10] },
        { t: 'link', r: [0, 0, 5, 5], target: '#page=3' },
        { t: 'untransform' },
      ],
    }
    const r = paintList(ctx, list, { makePath, background: false })
    expect(r.links).toEqual([
      { target: 'https://a', rect: { x: 1, y: 2, w: 3, h: 4 } },
      { target: '#page=3', rect: { x: 10, y: 10, w: 10, h: 10 } },
    ])
    expect(r.painted).toBe(0)
    expect(collectLinks(list)).toEqual(r.links)
    expect(hitLink(r.links, 15, 15).target).toBe('#page=3')
    expect(hitLink(r.links, 2, 3).target).toBe('https://a')
    expect(hitLink(r.links, 50, 50)).toBe(null)
  })
})

describe('collectNeeds', () => {
  it('gathers every font gid and image id', () => {
    const needs = collectNeeds({
      w: 1,
      h: 1,
      items: [
        { t: 'glyphs', font: 1, size: 10, c: '#000', g: [5, 0, 0, 6, 1, 0, 5, 2, 0] },
        { t: 'transform', m: [1, 0, 0, 1, 0, 0] },
        { t: 'glyphs', font: 2, size: 10, c: '#000', g: [7, 0, 0] },
        { t: 'glyphs', font: 1, size: 9, c: '#000', g: [8, 0, 0] },
        { t: 'image', image: 3, r: [0, 0, 1, 1] },
        { t: 'image', image: 3, r: [0, 0, 2, 2] },
        { t: 'untransform' },
      ],
    })
    expect([...needs.glyphs.keys()]).toEqual([1, 2])
    expect([...needs.glyphs.get(1)]).toEqual([5, 6, 8])
    expect([...needs.glyphs.get(2)]).toEqual([7])
    expect([...needs.images]).toEqual([3])
  })

  it('is empty for an empty list', () => {
    const needs = collectNeeds({ w: 1, h: 1, items: [] })
    expect(needs.glyphs.size).toBe(0)
    expect(needs.images.size).toBe(0)
  })
})

describe('GlyphCache', () => {
  it('knows what is missing and builds paths lazily', () => {
    let made = 0
    const gc = new GlyphCache({ makePath: (d) => (made++, { d }) })
    gc.add(JSON.stringify({ font: 1, upem: 1000, glyphs: { 36: 'M0 0L1 1Z' }, missing: [40] }))
    expect(gc.upem(1)).toBe(1000)
    expect(gc.upem(9)).toBe(1000)
    expect(gc.missing(1, [36, 40, 40, 41])).toEqual([40, 41])
    expect(gc.missing(2, new Set([1, 2]))).toEqual([1, 2])
    expect(gc.unavailable(1)).toEqual([40])
    expect(made).toBe(0)
    expect(gc.path(1, 36)).toEqual({ d: 'M0 0L1 1Z' })
    gc.path(1, 36)
    expect(made).toBe(1)
    expect(gc.path(1, 40)).toBe(undefined)
  })

  it('loads missing glyphs through the loader, sharing requests in flight', async () => {
    const asked = []
    const gc = new GlyphCache({
      makePath,
      loadGlyphs: async (font, gids) => {
        asked.push([font, [...gids]])
        return { font, upem: 2048, glyphs: Object.fromEntries(gids.filter((g) => g !== 13).map((g) => [g, `M${g} 0Z`])), missing: gids.filter((g) => g === 13) }
      },
    })
    const [a, b] = await Promise.all([gc.ensure(1, [5, 6, 13]), gc.ensure(1, [6, 7])])
    expect(asked).toEqual([[1, [5, 6, 13]], [1, [7]]])
    expect(a).toEqual([13])
    expect(b).toEqual([])
    expect(gc.upem(1)).toBe(2048)
    expect(gc.missing(1, [5, 6, 7])).toEqual([])
    await gc.ensure(1, [5, 6])
    expect(asked.length).toBe(2) // nothing to ask
  })

  it('without a loader ensure just answers what is missing', async () => {
    const gc = new GlyphCache({ makePath })
    expect(await gc.ensure(1, [1, 2])).toEqual([1, 2])
  })
})

describe('ImageCache', () => {
  it('loads each missing image once and decodes it', async () => {
    const asked = []
    const ic = new ImageCache({
      loadImage: async (id) => (asked.push(id), JSON.stringify({ image: id, width: 1, height: 1, rgba: 'AAAA/w==' })),
      decode: (j) => ({ id: j.image, px: [...decodeBase64(j.rgba)] }),
    })
    const [left] = await Promise.all([ic.ensure([3, 4, 3]), ic.ensure([4])])
    expect(left).toEqual([])
    expect(asked).toEqual([3, 4])
    expect(ic.get(3)).toEqual({ id: 3, px: [0, 0, 0, 255] })
    expect(ic.missing([3, 5])).toEqual([5])
  })

  it('closes bitmaps it lets go of', () => {
    let closed = 0
    const ic = new ImageCache()
    ic.set(1, { close: () => closed++ })
    ic.set(1, { close: () => closed++ })
    ic.clear()
    expect(closed).toBe(2)
  })
})

describe('decodeBase64', () => {
  it('decodes the RFC 4648 vectors', () => {
    const txt = (s) => String.fromCharCode(...decodeBase64(s))
    expect(txt('')).toBe('')
    expect(txt('Zg==')).toBe('f')
    expect(txt('Zm8=')).toBe('fo')
    expect(txt('Zm9v')).toBe('foo')
    expect(txt('Zm9vYmE=')).toBe('fooba')
    expect(txt('Zm9vYmFy')).toBe('foobar')
    expect([...decodeBase64('//79AA==')]).toEqual([0xff, 0xfe, 0xfd, 0x00])
  })
})
