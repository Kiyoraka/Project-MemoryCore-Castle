import { describe, expect, it, vi } from 'vitest'
import { applyOp, ask, docToLayer, layerToDoc, stripPixels, undoPill, SUNSET } from './ops.js'
import { durationFor, ease, normPoints, pathLength, pointAt, MAX_MS, MIN_MS } from './cursorPath.js'
import { makePixels, maskForSelection } from './engine.js'
import { compose2 } from './engine2.js'
import * as engine3 from './engine3.js'

const PNG = 'data:image/png;base64,AAAA'
const DREW = 'data:image/png;base64,DREW'

// ---------- a tiny world: 40 x 30, blue sky over green ground, a red box in the middle ----------

const W = 40
const H = 30
const SKY = [110, 160, 250, 255]
const GROUND = [60, 110, 40, 255]
const RED = [220, 30, 30, 255]

function scene() {
  const p = makePixels(W, H)
  for (let y = 0; y < H; y++) {
    for (let x = 0; x < W; x++) {
      const c = x >= 14 && x < 26 && y >= 9 && y < 24 ? RED : y < 12 ? SKY : GROUND
      p.data.set(c, (y * W + x) * 4)
    }
  }
  return p
}

const px = (p, x, y) => Array.from(p.data.slice((y * p.width + x) * 4, (y * p.width + x) * 4 + 4))

// Helpers as the view gives them: pngs and masks are ids into a little store.
function makeKit({ photo = scene(), selected = '' } = {}) {
  const store = new Map()
  const masks = new Map()
  let n = 0
  const log = []
  const keep = (p) => {
    const url = `data:image/png;base64,P${++n}`
    store.set(url, { data: new Uint8ClampedArray(p.data), width: p.width, height: p.height })
    return url
  }
  const photoPng = keep(photo)
  const pixelsOf = async (doc, id) => {
    const layer = doc.layers.find((l) => l.id === id)
    const p = store.get(layer?.png)
    if (!p) throw new Error(`no pixels for ${id}`)
    return { data: new Uint8ClampedArray(p.data), width: p.width, height: p.height }
  }
  const helpers = {
    log,
    store,
    masks,
    selectedId: selected,
    flatten: vi.fn(async () => 'FLAT'),
    generate: vi.fn(async () => ({ png: DREW, costUsd: 0.03 })),
    transform: vi.fn(async (doc, op) => ({ ...doc, width: op.w ?? doc.width, height: op.h ?? doc.height, layers: doc.layers.map((l) => ({ ...l, png: 'data:image/png;base64,CUT' })) })),
    pixelsOf: vi.fn(pixelsOf),
    maskOf: vi.fn(async (doc, id) => masks.get(doc.layers.find((l) => l.id === id)?.mask) || null),
    pngFrom: vi.fn(async (p) => {
      log.push('pngFrom')
      return keep(p)
    }),
    maskPngFrom: vi.fn(async (mask, w, h) => {
      expect(mask.length).toBe(w * h)
      const url = `data:image/png;base64,M${++n}`
      masks.set(url, new Uint8Array(mask))
      return url
    }),
    composite: vi.fn(async (doc) => {
      const layers = []
      for (const l of doc.layers) {
        const mask = l.mask ? masks.get(l.mask) : null
        if (l.kind === 'adjustment') layers.push({ ...l, mask })
        else if (l.kind === 'image') layers.push({ ...l, pixels: store.get(l.png), mask })
      }
      return compose2(layers, doc.width, doc.height)
    }),
    selectionMask: (doc) => {
      if (!doc.selection) return null
      if (doc.selection.kind === 'mask') return masks.get(doc.selection.png) || null
      return maskForSelection(doc.selection, doc.width, doc.height)
    },
    play: vi.fn(async (points, kind) => {
      log.push(`play:${kind}`)
    }),
  }
  return { helpers, photoPng }
}

function makeDoc(png = PNG) {
  return {
    width: W,
    height: H,
    layers: [{ id: 'photo', name: 'Photo', kind: 'image', visible: true, opacity: 1, blend: 'normal', x: 0, y: 0, png, mask: null }],
    adjust: { exposure: 0, brightness: 0, contrast: 0, highlights: 0, shadows: 0, warmth: 0, saturation: 0, sharpness: 0 },
    selection: null,
  }
}

function world(opts) {
  const kit = makeKit(opts)
  return { ...kit, doc: makeDoc(kit.photoPng) }
}

// runs ops in order and keeps the pills, as the view does
async function run(doc, ops, helpers) {
  const pills = []
  for (const op of ops) {
    const r = await applyOp(doc, op, helpers)
    pills.push({ id: pills.length + 1, label: r.label, undo: r.undo })
    doc = r.doc
  }
  return { doc, pills }
}

// ---------- v1, kept ----------

describe('stripPixels', () => {
  it('drops every png and mask, and says which layers are masked', () => {
    const doc = makeDoc()
    doc.selection = { kind: 'mask', png: PNG }
    doc.layers.push({ id: 'adj', name: 'Curves', kind: 'adjustment', visible: true, opacity: 1, blend: 'normal', x: 0, y: 0, mask: 'data:image/png;base64,M', adjustment: { type: 'levels', params: { black: 5, gamma: 1, white: 250 } } })
    const out = stripPixels(doc)
    expect(out.layers[0]).not.toHaveProperty('png')
    expect(out.layers[0]).not.toHaveProperty('masked')
    expect(out.layers[0].name).toBe('Photo')
    expect(out.layers[1]).not.toHaveProperty('mask')
    expect(out.layers[1].masked).toBe(true)
    expect(out.layers[1].adjustment.params.black).toBe(5)
    expect(out.selection).toEqual({ kind: 'mask' })
    expect(doc.layers[0].png).toBe(PNG)
    expect(doc.layers[1].mask).toBe('data:image/png;base64,M')
  })
})

describe('adjust', () => {
  it('adds relative values, clamps them and labels them', async () => {
    const doc = makeDoc()
    doc.adjust.warmth = 95
    const r = await applyOp(doc, { op: 'adjust', values: { warmth: 18, shadows: 12, bogus: 5 }, relative: true })
    expect(r.doc.adjust.warmth).toBe(100)
    expect(r.doc.adjust.shadows).toBe(12)
    expect(r.doc.adjust).not.toHaveProperty('bogus')
    expect(r.label).toBe('Warmth +18, Shadows +12')
    expect(r.undo).toEqual({ kind: 'adjust', before: { warmth: 95, shadows: 0 } })
    expect(doc.adjust.warmth).toBe(95)
  })

  it('sets absolute values, clamped, and throws on unknown keys', async () => {
    const r = await applyOp(makeDoc(), { op: 'adjust', values: { contrast: -250 }, relative: false })
    expect(r.doc.adjust.contrast).toBe(-100)
    expect(r.label).toBe('Contrast -100')
    await expect(applyOp(makeDoc(), { op: 'adjust', values: { nope: 3 } })).rejects.toThrow()
  })

  it('undo puts back only its own keys', async () => {
    const { doc, pills } = await run(makeDoc(), [
      { op: 'adjust', values: { warmth: 20 }, relative: true },
      { op: 'adjust', values: { saturation: 10 }, relative: true },
    ])
    const back = undoPill(doc, pills[0])
    expect(back.doc.adjust.warmth).toBe(0)
    expect(back.doc.adjust.saturation).toBe(10)
    expect(back.dropAfter).toBe(false)
  })
})

describe('text and layer', () => {
  it('adds a text layer on top and undo removes it', async () => {
    const r = await applyOp(makeDoc(), { op: 'text', value: 'Summer', x: 4, y: 2000, size: 64, color: '#ffcc00', font: 'Inter' })
    const top = r.doc.layers.at(-1)
    expect(top.kind).toBe('text')
    expect(top.text).toMatchObject({ value: 'Summer', size: 64, color: '#ffcc00' })
    expect(top.y).toBe(H)
    expect(r.label).toBe('Text added')
    expect(undoPill(r.doc, { undo: r.undo }).doc.layers).toHaveLength(1)
    await expect(applyOp(makeDoc(), { op: 'text', value: '  ' })).rejects.toThrow()
  })

  it('finds by name without case and restores on undo', async () => {
    const r = await applyOp(makeDoc(), { op: 'layer', name: 'PHOTO', set: { opacity: 60, blend: 'multiply' } })
    expect(r.doc.layers[0].opacity).toBe(0.6)
    expect(r.label).toBe('Layer: opacity 60%, blend multiply')
    expect(undoPill(r.doc, { undo: r.undo }).doc.layers[0]).toMatchObject({ opacity: 1, blend: 'normal' })
    await expect(applyOp(makeDoc(), { op: 'layer', name: 'ghost', set: { visible: false } })).rejects.toThrow()
  })
})

describe('whole-document ops', () => {
  it('crop goes through transform and undo returns the snapshot, dropping later pills', async () => {
    const { helpers } = makeKit()
    const { doc, pills } = await run(makeDoc(), [
      { op: 'adjust', values: { warmth: 10 }, relative: true },
      { op: 'crop', x: 10, y: 5, w: 20, h: 15 },
      { op: 'adjust', values: { contrast: 5 }, relative: true },
    ], helpers)
    expect(helpers.transform).toHaveBeenCalledWith(expect.any(Object), { op: 'crop', x: 10, y: 5, w: 20, h: 15 })
    expect(doc.width).toBe(20)
    expect(pills[1].label).toBe('Cropped to 20×15')
    const back = undoPill(doc, pills[1])
    expect(back.dropAfter).toBe(true)
    expect(back.doc.width).toBe(W)
    expect(back.doc.adjust.warmth).toBe(10)
  })

  it('checks rotate, flip and resize', async () => {
    const { helpers: h } = makeKit()
    expect((await applyOp(makeDoc(), { op: 'rotate', turns: 1 }, h)).label).toBe('Rotated 90°')
    expect((await applyOp(makeDoc(), { op: 'flip', axis: 'v' }, h)).label).toBe('Flipped vertically')
    expect((await applyOp(makeDoc(), { op: 'resize', width: 500.4, height: 300 }, h)).label).toBe('Resized to 500×300')
    await expect(applyOp(makeDoc(), { op: 'rotate', turns: 4 }, h)).rejects.toThrow()
    await expect(applyOp(makeDoc(), { op: 'flip', axis: 'x' }, h)).rejects.toThrow()
  })
})

// ---------- v2: adjustment layers ----------

describe('adjustment layers', () => {
  it('curves, levels and hsl each add a layer on top; undo removes only its own', async () => {
    const { doc: start, helpers } = world()
    const { doc, pills } = await run(start, [
      { op: 'curves', channel: 'rgb', points: [[255, 255], [0, -5], [64, 50], [64, 40], [300, 255]] },
      { op: 'levels', black: 10, gamma: 1.234, white: 245 },
      { op: 'hsl', range: 'blues', sat: 20 },
    ], helpers)
    expect(pills.map((p) => p.label)).toEqual(['Curves', 'Levels', 'Hue/Saturation'])
    const [, curves, levels, hsl] = doc.layers
    expect(curves).toMatchObject({ kind: 'adjustment', mask: null, adjustment: { type: 'curves', params: { rgb: [[0, 0], [64, 40], [255, 255]] } } })
    expect(levels.adjustment).toEqual({ type: 'levels', params: { black: 10, gamma: 1.23, white: 245 } })
    expect(hsl.adjustment).toEqual({ type: 'hsl', params: { ranges: { blues: { hue: 0, sat: 20, light: 0 } } } })
    const back = undoPill(doc, pills[1])
    expect(back.doc.layers.map((l) => l.name)).toEqual(['Photo', 'Curves', 'Hue/Saturation'])
  })

  it('a single channel keeps rgb straight, and the selection becomes its mask', async () => {
    const { doc: start, helpers } = world()
    start.selection = { kind: 'rect', x: 0, y: 0, w: 10, h: 10 }
    const r = await applyOp(start, { op: 'curves', channel: 'b', points: [[0, 20], [255, 230]] }, helpers)
    const layer = r.doc.layers.at(-1)
    expect(layer.adjustment.params).toEqual({ rgb: [[0, 0], [255, 255]], b: [[0, 20], [255, 230]] })
    const mask = helpers.masks.get(layer.mask)
    expect(mask[0]).toBe(255)
    expect(mask[15 * W + 20]).toBe(0)
  })

  it('refuses what changes nothing', async () => {
    const { doc, helpers } = world()
    await expect(applyOp(doc, { op: 'curves', points: [[3, 3]] }, helpers)).rejects.toThrow()
    await expect(applyOp(doc, { op: 'levels' }, helpers)).rejects.toThrow()
    await expect(applyOp(doc, { op: 'hsl', range: 'reds' }, helpers)).rejects.toThrow()
  })

  it('levels clamp white above black and gamma into range', async () => {
    const { doc, helpers } = world()
    const r = await applyOp(doc, { op: 'levels', black: 250, white: 100, gamma: 50 }, helpers)
    expect(r.doc.layers.at(-1).adjustment.params).toEqual({ black: 250, gamma: 9.99, white: 252 })
  })
})

// ---------- v2: pixel ops ----------

describe('filter', () => {
  it('blurs only inside the selection and undo brings back the old png', async () => {
    const { doc: start, helpers, photoPng } = world()
    start.selection = { kind: 'rect', x: 10, y: 5, w: 20, h: 20 }
    const r = await applyOp(start, { op: 'filter', name: 'blur', amount: 40 }, helpers)
    expect(r.label).toBe('Blur 40')
    expect(r.undo).toEqual({ kind: 'layerPng', id: 'photo', before: photoPng })
    const out = helpers.store.get(r.doc.layers[0].png)
    expect(px(out, 0, 0)).toEqual(SKY)
    expect(px(out, 39, 29)).toEqual(GROUND)
    expect(px(out, 14, 12)).not.toEqual(RED)
    expect(undoPill(r.doc, { undo: r.undo }).doc.layers[0].png).toBe(photoPng)
    expect(start.layers[0].png).toBe(photoPng)
  })

  it('every filter runs, and unknown or zero ones are refused', async () => {
    const { doc, helpers } = world()
    for (const name of ['sharpen', 'noise', 'vignette', 'denoise']) {
      const r = await applyOp(doc, { op: 'filter', name, amount: 60 }, helpers)
      expect(r.undo.kind).toBe('layerPng')
    }
    await expect(applyOp(doc, { op: 'filter', name: 'liquify', amount: 50 }, helpers)).rejects.toThrow()
    await expect(applyOp(doc, { op: 'filter', name: 'blur', amount: 0 }, helpers)).rejects.toThrow()
  })

  it('works on the layer he has selected', async () => {
    const { doc: start, helpers } = world({ selected: 'photo' })
    start.layers.push({ id: 'top', name: 'Top', kind: 'image', visible: true, opacity: 1, blend: 'normal', x: 0, y: 0, png: start.layers[0].png, mask: null })
    const r = await applyOp(start, { op: 'filter', name: 'blur', amount: 30 }, helpers)
    expect(r.undo.id).toBe('photo')
  })
})

describe('brush', () => {
  it('her cursor plays first, then a new paint layer is drawn along the points', async () => {
    const { doc, helpers } = world()
    const points = [[2, 15], [37, 15]]
    const r = await applyOp(doc, { op: 'brush', points, size: 6, color: '#ff00ff', opacity: 1, hardness: 1 }, helpers)
    expect(helpers.play).toHaveBeenCalledWith(points, 'brush')
    expect(helpers.log.indexOf('play:brush')).toBeLessThan(helpers.log.indexOf('pngFrom'))
    const top = r.doc.layers.at(-1)
    expect(top).toMatchObject({ name: 'Companion - paint', kind: 'image', x: 0, y: 0, mask: null })
    expect(r.label).toBe('Brush stroke')
    expect(r.undo).toEqual({ kind: 'removeLayer', id: top.id })
    const paint = helpers.store.get(top.png)
    expect([paint.width, paint.height]).toEqual([W, H])
    expect(px(paint, 20, 15)).toEqual([255, 0, 255, 255])
    expect(px(paint, 20, 2)[3]).toBe(0)
    expect(undoPill(r.doc, { undo: r.undo }).doc.layers).toHaveLength(1)
  })

  it('is clipped to the selection and never builds past its opacity', async () => {
    const { doc: start, helpers } = world()
    start.selection = { kind: 'rect', x: 0, y: 0, w: 20, h: 30 }
    const r = await applyOp(start, { op: 'brush', points: [[2, 15], [37, 15], [2, 15]], size: 8, color: '#000000', opacity: 0.5, hardness: 1 }, helpers)
    const paint = helpers.store.get(r.doc.layers.at(-1).png)
    expect(px(paint, 10, 15)[3]).toBe(128)
    expect(px(paint, 30, 15)[3]).toBe(0)
  })

  it('paints into a named layer with a layerPng undo', async () => {
    const { doc, helpers, photoPng } = world()
    const r = await applyOp(doc, { op: 'brush', points: [[5, 5]], size: 4, color: '#000000', layer: 'photo' }, helpers)
    expect(r.doc.layers).toHaveLength(1)
    expect(r.undo).toEqual({ kind: 'layerPng', id: 'photo', before: photoPng })
    expect(px(helpers.store.get(r.doc.layers[0].png), 5, 5).slice(0, 3)).toEqual([0, 0, 0])
    await expect(applyOp(doc, { op: 'brush', points: [] }, helpers)).rejects.toThrow()
  })
})

describe('gradient and shape', () => {
  it('a gradient plays from -> to, then lands on its own layer', async () => {
    const { doc, helpers } = world()
    const r = await applyOp(doc, { op: 'gradient', kind: 'linear', from: [0, 0], to: [40, 0], stops: [{ at: 0, color: '#000000' }, { at: 1, color: '#ffffff', alpha: 1 }] }, helpers)
    expect(helpers.play).toHaveBeenCalledWith([[0, 0], [40, 0]], 'gradient')
    const top = r.doc.layers.at(-1)
    expect(top.name).toBe('Companion - gradient')
    const g = helpers.store.get(top.png)
    expect(px(g, 0, 10)[0]).toBeLessThan(20)
    expect(px(g, 39, 10)[0]).toBeGreaterThan(235)
    expect(r.label).toBe('Gradient')
    expect(undoPill(r.doc, { undo: r.undo }).doc.layers).toHaveLength(1)
    await expect(applyOp(doc, { op: 'gradient', stops: [{ at: 0, color: '#000000' }] }, helpers)).rejects.toThrow()
  })

  it('a rect plays its outline and is drawn; a shape needs a fill or a stroke', async () => {
    const { doc, helpers } = world()
    const r = await applyOp(doc, { op: 'shape', kind: 'rect', x: 5, y: 5, w: 10, h: 10, fill: '#00ff00' }, helpers)
    expect(helpers.play).toHaveBeenCalledWith([[5, 5], [15, 5], [15, 15], [5, 15], [5, 5]], 'shape')
    const top = r.doc.layers.at(-1)
    expect(top.name).toBe('Companion - shape')
    expect(px(helpers.store.get(top.png), 10, 10)).toEqual([0, 255, 0, 255])
    expect(r.label).toBe('Rectangle')
    const poly = await applyOp(doc, { op: 'shape', kind: 'polygon', points: [[1, 1], [10, 1], [5, 9]], stroke: '#ffffff', width: 2 }, helpers)
    expect(helpers.play).toHaveBeenLastCalledWith([[1, 1], [10, 1], [5, 9], [1, 1]], 'shape')
    expect(poly.label).toBe('Shape')
    await expect(applyOp(doc, { op: 'shape', kind: 'rect', x: 1, y: 1, w: 5, h: 5 }, helpers)).rejects.toThrow()
  })
})

describe('transform', () => {
  it('scales a layer about its centre; undo restores its pixels and place', async () => {
    const { doc, helpers, photoPng } = world()
    const r = await applyOp(doc, { op: 'transform', layer: 'Photo', scale: 0.5 }, helpers)
    const layer = r.doc.layers[0]
    const out = helpers.store.get(layer.png)
    // 40 x 30 halved about (20, 15): x 10..30, y 7.5..22.5, grown to whole pixels
    expect([out.width, out.height]).toEqual([20, 16])
    expect([layer.x, layer.y]).toEqual([10, 7])
    expect(r.label).toBe('Scaled 50%')
    expect(r.undo).toEqual({ kind: 'layerPng', id: 'photo', before: photoPng, x: 0, y: 0 })
    const back = undoPill(r.doc, { undo: r.undo }).doc.layers[0]
    expect([back.png, back.x, back.y]).toEqual([photoPng, 0, 0])
  })

  it('moves and turns, and refuses a transform that changes nothing', async () => {
    const { doc, helpers } = world()
    const moved = await applyOp(doc, { op: 'transform', layer: 'photo', dx: 5, dy: -3 }, helpers)
    expect([moved.doc.layers[0].x, moved.doc.layers[0].y]).toEqual([5, -3])
    expect(moved.label).toBe('Moved')
    const turned = await applyOp(doc, { op: 'transform', layer: 'photo', rotate: 90 }, helpers)
    const t = helpers.store.get(turned.doc.layers[0].png)
    expect([t.width, t.height]).toEqual([H, W])
    await expect(applyOp(doc, { op: 'transform', layer: 'photo' }, helpers)).rejects.toThrow()
  })
})

describe('masks', () => {
  it('from the selection, inverted, cleared - each undo on its own', async () => {
    const { doc: start, helpers } = world()
    start.selection = { kind: 'rect', x: 0, y: 0, w: 10, h: 30 }
    const { doc, pills } = await run(start, [
      { op: 'mask', layer: 'Photo', from: 'selection' },
      { op: 'mask', layer: 'Photo', from: 'invert' },
    ], helpers)
    expect(pills.map((p) => p.label)).toEqual(['Mask from selection', 'Mask inverted'])
    const inverted = helpers.masks.get(doc.layers[0].mask)
    expect(inverted[0]).toBe(0)
    expect(inverted[20]).toBe(255)
    expect(pills[0].undo).toEqual({ kind: 'layerMask', id: 'photo', before: null })
    const back = undoPill(doc, pills[1])
    expect(helpers.masks.get(back.doc.layers[0].mask)[0]).toBe(255)
    const cleared = await applyOp(doc, { op: 'mask', from: 'clear', layer: 'photo' }, helpers)
    expect(cleared.doc.layers[0].mask).toBe(null)
    expect(undoPill(cleared.doc, { undo: cleared.undo }).doc.layers[0].mask).toBe(doc.layers[0].mask)
    await expect(applyOp(makeDoc(), { op: 'mask', from: 'clear', layer: 'photo' }, helpers)).rejects.toThrow()
  })

  it('from the sky and the subject, read from the whole picture', async () => {
    const { doc, helpers } = world()
    const sky = await applyOp(doc, { op: 'mask', from: 'sky' }, helpers)
    const m = helpers.masks.get(sky.doc.layers[0].mask)
    expect(m[2 * W + 2]).toBe(255)
    expect(m[28 * W + 2]).toBe(0)
    const subject = await applyOp(doc, { op: 'mask', layer: 'photo', from: 'subject' }, helpers)
    const s = helpers.masks.get(subject.doc.layers[0].mask)
    expect(s[16 * W + 20]).toBe(255)
    expect(s[1 * W + 1]).toBe(0)
  })
})

describe('select', () => {
  it('a rect plays its outline, inside the picture; undo restores the old selection', async () => {
    const { doc: start, helpers } = world()
    start.selection = { kind: 'rect', x: 1, y: 1, w: 2, h: 2 }
    const r = await applyOp(start, { op: 'select', x: 30, y: -10, w: 500, h: 20 }, helpers)
    expect(r.doc.selection).toEqual({ kind: 'rect', x: 30, y: 0, w: 10, h: 20 })
    expect(helpers.play).toHaveBeenCalledWith([[30, 0], [40, 0], [40, 20], [30, 20], [30, 0]], 'select')
    expect(undoPill(r.doc, { undo: r.undo }).doc.selection).toEqual(start.selection)
    const nested = await applyOp(start, { op: 'select', rect: { x: 2, y: 2, w: 4, h: 4 } }, helpers)
    expect(nested.doc.selection).toEqual({ kind: 'rect', x: 2, y: 2, w: 4, h: 4 })
  })

  it('subject, sky, colour, invert and none', async () => {
    const { doc, helpers } = world()
    const sky = await applyOp(doc, { op: 'select', sky: true }, helpers)
    expect(sky.label).toBe('Sky selected')
    expect(sky.doc.selection.kind).toBe('mask')
    expect(helpers.masks.get(sky.doc.selection.png)[0]).toBe(255)
    const inv = await applyOp(sky.doc, { op: 'select', invert: true }, helpers)
    expect(inv.label).toBe('Selection inverted')
    expect(helpers.masks.get(inv.doc.selection.png)[0]).toBe(0)
    const red = await applyOp(doc, { op: 'select', color: '#dc1e1e', fuzz: 20 }, helpers)
    const rm = helpers.masks.get(red.doc.selection.png)
    expect([rm[16 * W + 20], rm[0]]).toEqual([255, 0])
    const subject = await applyOp(doc, { op: 'select', subject: true }, helpers)
    expect(subject.label).toBe('Subject selected')
    const none = await applyOp(sky.doc, { op: 'select', none: true }, helpers)
    expect(none.doc.selection).toBe(null)
    expect(undoPill(none.doc, { undo: none.undo }).doc.selection).toEqual(sky.doc.selection)
    await expect(applyOp(doc, { op: 'select', invert: true }, helpers)).rejects.toThrow()
    await expect(applyOp(doc, { op: 'select', color: '#ff00ff', fuzz: 0 }, helpers)).rejects.toThrow()
  })
})

describe('retouch', () => {
  it('heal plays the stroke, then mends the top picture layer', async () => {
    const spot = scene()
    spot.data.set([0, 0, 0, 255], (4 * W + 4) * 4)
    const { helpers: h2, photoPng: png2 } = makeKit({ photo: spot })
    const doc = makeDoc(png2)
    const r = await applyOp(doc, { op: 'heal', points: [[4, 4]], size: 4 }, h2)
    expect(h2.play).toHaveBeenCalledWith([[4, 4]], 'brush')
    expect(h2.log.indexOf('play:brush')).toBeLessThan(h2.log.indexOf('pngFrom'))
    expect(r.label).toBe('Healed')
    expect(r.undo).toEqual({ kind: 'layerPng', id: 'photo', before: png2 })
    const healed = px(h2.store.get(r.doc.layers[0].png), 4, 4)
    expect(healed[2]).toBeGreaterThan(150)
  })

  it('fill content_aware rebuilds the selection; erase rebuilds its box', async () => {
    const { doc: start, helpers, photoPng } = world()
    start.selection = { kind: 'rect', x: 14, y: 9, w: 12, h: 3 }
    const filled = await applyOp(start, { op: 'fill', mode: 'content_aware' }, helpers)
    expect(filled.label).toBe('Erased')
    expect(filled.undo).toEqual({ kind: 'layerPng', id: 'photo', before: photoPng })
    const fp = helpers.store.get(filled.doc.layers[0].png)
    expect(px(fp, 20, 10)).not.toEqual(RED)
    expect(px(fp, 0, 0)).toEqual(SKY)
    const erased = await applyOp(makeDoc(photoPng), { op: 'erase', x: 14, y: 9, w: 12, h: 15 }, helpers)
    expect(erased.label).toBe('Erased')
    expect(px(helpers.store.get(erased.doc.layers[0].png), 20, 16)).not.toEqual(RED)
    expect(helpers.generate).not.toHaveBeenCalled()
    await expect(applyOp(makeDoc(photoPng), { op: 'fill', mode: 'content_aware' }, helpers)).rejects.toThrow('select what to fill first')
    const boxed = await applyOp(makeDoc(photoPng), { op: 'fill', mode: 'content_aware', x: 14, y: 9, w: 4, h: 4 }, helpers)
    expect(boxed.undo.kind).toBe('layerPng')
  })
})

// ---------- v2: native replacements, and the one OpenAI op ----------

describe('native remove_background and replace_sky', () => {
  it('remove_background sets a mask on the photo, never asks OpenAI; undo clears it', async () => {
    const { doc, helpers } = world()
    const r = await applyOp(doc, { op: 'remove_background' }, helpers)
    expect(helpers.generate).not.toHaveBeenCalled()
    expect(helpers.flatten).not.toHaveBeenCalled()
    expect(r.label).toBe('Background removed')
    expect(r.doc.layers).toHaveLength(1)
    const mask = helpers.masks.get(r.doc.layers[0].mask)
    expect(mask[16 * W + 20]).toBe(255)
    expect(mask[0]).toBe(0)
    expect(r.undo).toEqual({ kind: 'layerMask', id: 'photo', before: null })
    expect(undoPill(r.doc, { undo: r.undo }).doc.layers[0].mask).toBe(null)
    expect(doc.layers[0].mask).toBe(null)
  })

  it('remove_background on an offset layer places the mask in the document', async () => {
    const { doc, helpers } = world()
    doc.layers[0].x = 5
    const r = await applyOp(doc, { op: 'remove_background', layer: 'Photo' }, helpers)
    const mask = helpers.masks.get(r.doc.layers[0].mask)
    expect(mask[16 * W + 25]).toBe(255)
    expect(mask[16 * W + 2]).toBe(0)
  })

  it('replace_sky adds a gradient layer masked by the sky, under the text; undo removes it', async () => {
    const { doc: start, helpers } = world()
    start.layers.push({ id: 't', name: 'Hi', kind: 'text', visible: true, opacity: 1, blend: 'normal', x: 0, y: 0, text: { value: 'Hi' } })
    const r = await applyOp(start, { op: 'replace_sky' }, helpers)
    expect(helpers.generate).not.toHaveBeenCalled()
    expect(r.label).toBe('Sky replaced')
    expect(r.doc.layers.map((l) => l.name)).toEqual(['Photo', 'Companion - sky', 'Hi'])
    const sky = r.doc.layers[1]
    const mask = helpers.masks.get(sky.mask)
    expect(mask[0]).toBe(255)
    expect(mask[28 * W + 0]).toBe(0)
    const g = helpers.store.get(sky.png)
    // the top of the sky is the first colour (blue), the horizon the last (orange)
    const [tr, , tb] = px(g, 0, 0)
    const [br, , bb] = px(g, 0, 11)
    expect(SUNSET[0]).toBe('#7fb6ff')
    expect(tb).toBeGreaterThan(240)
    expect(tr).toBeLessThan(150)
    expect(br).toBeGreaterThan(240)
    expect(bb).toBeLessThan(120)
    expect(undoPill(r.doc, { undo: r.undo }).doc.layers.map((l) => l.name)).toEqual(['Photo', 'Hi'])
    const own = await applyOp(start, { op: 'replace_sky', colors: ['#000000', '#ffffff'] }, helpers)
    expect(px(helpers.store.get(own.doc.layers[1].png), 0, 0)[0]).toBeLessThan(20)
  })
})

describe('generate_layer, the only OpenAI op', () => {
  it('adds a drawn layer on top with the cost in the label', async () => {
    const { doc, helpers } = world()
    const r = await applyOp(doc, { op: 'generate_layer', prompt: 'a small red balloon floating high' }, helpers)
    expect(helpers.generate).toHaveBeenCalledWith({ kind: 'generate_layer', image: 'FLAT', prompt: 'a small red balloon floating high' })
    expect(r.doc.layers.map((l) => l.name)).toEqual(['Photo', 'Companion - a small red balloon'])
    expect(r.doc.layers[1].png).toBe(DREW)
    expect(r.label).toBe('Drawn by your image provider · $0.03')
    expect(undoPill(r.doc, { undo: r.undo }).doc.layers).toHaveLength(1)
    await expect(applyOp(doc, { op: 'generate_layer' }, helpers)).rejects.toThrow()
  })

  it('throws when no picture comes back', async () => {
    const { doc, helpers } = world()
    helpers.generate = async () => ({ png: '', costUsd: 0 })
    await expect(applyOp(doc, { op: 'generate_layer', prompt: 'x' }, helpers)).rejects.toThrow()
  })

  it('a v1 generated pill still undoes', () => {
    const doc = makeDoc()
    doc.layers[0].visible = false
    doc.layers.push({ id: 'alice-2', name: 'Companion - sky', kind: 'image', visible: true, png: PNG })
    const back = undoPill(doc, { undo: { kind: 'generated', newId: 'alice-2', hidden: ['photo'] } })
    expect(back.doc.layers.map((l) => [l.id, l.visible])).toEqual([['photo', true]])
  })
})

describe('independent undo across her new tools', () => {
  it('a middle pill undoes on its own after later ones', async () => {
    const { doc: start, helpers } = world()
    const { doc, pills } = await run(start, [
      { op: 'remove_background' },
      { op: 'curves', points: [[0, 10], [255, 245]] },
      { op: 'brush', points: [[3, 3], [9, 3]], size: 3, color: '#ffffff' },
      { op: 'adjust', values: { saturation: 10 }, relative: true },
    ], helpers)
    expect(pills.map((p) => p.label)).toEqual(['Background removed', 'Curves', 'Brush stroke', 'Saturation +10'])
    const back = undoPill(doc, pills[1])
    expect(back.dropAfter).toBe(false)
    expect(back.doc.layers.map((l) => l.name)).toEqual(['Photo', 'Companion - paint'])
    expect(back.doc.layers[0].mask).toBe(doc.layers[0].mask)
    expect(back.doc.adjust.saturation).toBe(10)
    const again = undoPill(back.doc, pills[0])
    expect(again.doc.layers[0].mask).toBe(null)
    expect(again.doc.layers).toHaveLength(2)
  })
})

describe('engines are injectable', () => {
  it('uses the engines handed in', async () => {
    const { doc, helpers } = world()
    const blurred = makePixels(W, H, [1, 2, 3, 255])
    const engines = { gaussianBlur: vi.fn(() => blurred) }
    const r = await applyOp(doc, { op: 'filter', name: 'blur', amount: 50 }, helpers, engines)
    expect(engines.gaussianBlur).toHaveBeenCalledWith(expect.objectContaining({ width: W, height: H }), 7.5, null)
    expect(px(helpers.store.get(r.doc.layers[0].png), 0, 0)).toEqual([1, 2, 3, 255])
  })
})

describe('mask geometry', () => {
  it('docToLayer and layerToDoc place a mask by the layer offset', () => {
    const doc = new Uint8Array(4 * 3)
    doc[1 * 4 + 2] = 255
    const local = docToLayer(doc, 4, 3, { x: 1, y: 1 }, 2, 2)
    expect(Array.from(local)).toEqual([0, 255, 0, 0])
    expect(Array.from(layerToDoc(local, 2, 2, { x: 1, y: 1 }, 4, 3))).toEqual(Array.from(doc))
  })
})

// ---------- v3: groups, clipping, blend, styles ----------

const BLEND27 = [
  'normal', 'dissolve', 'darken', 'multiply', 'colorBurn', 'linearBurn', 'darkerColor', 'lighten', 'screen', 'colorDodge',
  'linearDodge', 'lighterColor', 'overlay', 'softLight', 'hardLight', 'vividLight', 'linearLight', 'pinLight', 'hardMix',
  'difference', 'exclusion', 'subtract', 'divide', 'hue', 'saturation', 'color', 'luminosity',
]

// pixels filled with a colour where the (layer-sized) mask is set, everywhere when there is none
function painted(p, rgba, mask = null) {
  const out = { data: new Uint8ClampedArray(p.data), width: p.width, height: p.height }
  for (let j = 0; j < p.width * p.height; j++) if (!mask || mask[j]) out.data.set(rgba, j * 4)
  return out
}

function rectMaskOf(w, h, r) {
  const m = new Uint8Array(w * h)
  for (let y = r.y; y < r.y + r.h; y++) for (let x = r.x; x < r.x + r.w; x++) m[y * w + x] = 255
  return m
}

// engine3 as a test double: each tool leaves a mark we can read back
function mockE3() {
  const brush = (rgba) => vi.fn((p, spec, mask) => painted(p, rgba, mask))
  return {
    BLEND_MODES: BLEND27,
    LOOKS: { warmFilm: {}, coolFade: {} },
    artboardPresets: [{ key: 'icon-1024', name: 'App icon', w: 1024, h: 1024 }, { key: 'ig-post', name: 'Instagram post', w: 1080, h: 1080 }],
    motionBlur: brush([9, 9, 9, 255]),
    pixelate: brush([8, 8, 8, 255]),
    liquifyMesh: vi.fn((w, h) => ({ w, h, strokes: [] })),
    liquifyStroke: vi.fn((mesh, s) => ({ ...mesh, strokes: [...mesh.strokes, s] })),
    applyMesh: vi.fn((p, mesh) => painted(p, [mesh.strokes.length, 0, 0, 255])),
    meshWarp: vi.fn((p, grid, o) => makePixels(o.width, o.height, [1, 1, 1, 255])),
    perspective: vi.fn((p, corners, o) => makePixels(o.width, o.height, [2, 2, 2, 255])),
    dodge: brush([250, 250, 250, 255]),
    burn: brush([5, 5, 5, 255]),
    sponge: brush([100, 100, 100, 255]),
    smudge: brush([101, 101, 101, 255]),
    blurBrush: brush([102, 102, 102, 255]),
    sharpenBrush: brush([103, 103, 103, 255]),
    spotHeal: brush([104, 104, 104, 255]),
    patch: vi.fn((p, hole) => painted(p, [105, 105, 105, 255], hole)),
    quickSelect: vi.fn((comp) => rectMaskOf(comp.width, comp.height, { x: 0, y: 0, w: comp.width / 2, h: comp.height })),
    objectSelect: vi.fn((comp, rect) => rectMaskOf(comp.width, comp.height, rect)),
  }
}

const v3 = (doc, op, helpers, E3 = mockE3()) => applyOp(doc, op, helpers, undefined, E3)

function layered() {
  const doc = makeDoc()
  const extra = (id) => ({ id, name: id.toUpperCase(), kind: 'image', visible: true, opacity: 1, blend: 'normal', x: 0, y: 0, png: PNG, mask: null })
  doc.layers.push(extra('a'), extra('b'), extra('c'))
  return doc
}

const order = (doc) => doc.layers.map((l) => l.id)
const parents = (doc) => doc.layers.map((l) => l.parent ?? null)

describe('v3 group and ungroup', () => {
  it('groups layers together under a new group placed where the topmost was; undo puts them back', async () => {
    const doc = layered()
    const r = await applyOp(doc, { op: 'group', layers: ['A', 'c'], name: 'Pair' })
    const g = r.doc.layers.at(-1)
    expect(g).toMatchObject({ kind: 'group', name: 'Pair', collapsed: false, visible: true, opacity: 1 })
    expect(order(r.doc)).toEqual(['photo', 'b', 'a', 'c', g.id])
    expect(r.doc.layers.find((l) => l.id === 'a').parent).toBe(g.id)
    expect(r.doc.layers.find((l) => l.id === 'c').parent).toBe(g.id)
    expect(r.doc.layers.find((l) => l.id === 'b').parent).toBeUndefined()
    expect(r.label).toBe('Grouped 2 layers')
    expect(order(doc)).toEqual(['photo', 'a', 'b', 'c'])
    // a later op stays when the group is undone on its own
    const t = await applyOp(r.doc, { op: 'text', value: 'Hi' })
    const back = undoPill(t.doc, { undo: r.undo })
    expect(order(back.doc)).toEqual(['photo', 'a', 'b', 'c', t.doc.layers.at(-1).id])
    expect(parents(back.doc).every((p) => p === null)).toBe(true)
    await expect(applyOp(doc, { op: 'group', layers: ['ghost'] })).rejects.toThrow('there is no layer called ghost')
    await expect(applyOp(doc, { op: 'group', layers: [] })).rejects.toThrow()
  })

  it('a group inside a group carries its layers along, and ungroup round-trips', async () => {
    const doc = layered()
    const inner = await applyOp(doc, { op: 'group', layers: ['a', 'b'] })
    const innerGroup = inner.doc.layers.find((l) => l.kind === 'group')
    const innerId = innerGroup.id
    expect(innerGroup.name).toBe('Group 1')
    expect(order(inner.doc)).toEqual(['photo', 'a', 'b', innerId, 'c'])
    const outer = await applyOp(inner.doc, { op: 'group', layers: [innerId, 'c'], name: 'Outer' })
    const outerId = outer.doc.layers.at(-1).id
    expect(order(outer.doc)).toEqual(['photo', 'a', 'b', innerId, 'c', outerId])
    expect(outer.doc.layers.find((l) => l.id === innerId).parent).toBe(outerId)
    expect(outer.doc.layers.find((l) => l.id === 'a').parent).toBe(innerId)

    const un = await applyOp(outer.doc, { op: 'ungroup', group: 'Outer' })
    expect(un.label).toBe('Ungrouped Outer')
    expect(un.doc.layers.some((l) => l.id === outerId)).toBe(false)
    expect(un.doc.layers.find((l) => l.id === innerId).parent).toBeUndefined()
    expect(un.doc.layers.find((l) => l.id === 'a').parent).toBe(innerId)
    const re = undoPill(un.doc, { undo: un.undo })
    expect(order(re.doc)).toEqual(order(outer.doc))
    expect(parents(re.doc)).toEqual(parents(outer.doc))
    await expect(applyOp(doc, { op: 'ungroup', group: 'a' })).rejects.toThrow('there is no group called a')
  })
})

describe('v3 clip, blend and style', () => {
  it('clip sets clipped with a layerFields undo; the bottom layer cannot clip', async () => {
    const doc = layered()
    const r = await applyOp(doc, { op: 'clip', layer: 'b', on: true })
    expect(r.doc.layers.find((l) => l.id === 'b').clipped).toBe(true)
    expect(r.label).toBe('Clipped B')
    expect(r.undo).toEqual({ kind: 'layerFields', id: 'b', before: { clipped: false } })
    expect(undoPill(r.doc, { undo: r.undo }).doc.layers.find((l) => l.id === 'b').clipped).toBe(false)
    const off = await applyOp(r.doc, { op: 'clip', layer: 'b', on: false })
    expect(off.label).toBe('Unclipped B')
    await expect(applyOp(doc, { op: 'clip', layer: 'photo', on: true })).rejects.toThrow()
    await expect(applyOp(doc, { op: 'clip', layer: 'b', on: false })).rejects.toThrow()
  })

  it('blend takes any of the 27 modes; the v2 layer op takes them too', async () => {
    const doc = layered()
    const r = await v3(doc, { op: 'blend', layer: 'a', mode: 'colorDodge' })
    expect(r.doc.layers.find((l) => l.id === 'a').blend).toBe('colorDodge')
    expect(r.label).toBe('Blend colorDodge')
    expect(undoPill(r.doc, { undo: r.undo }).doc.layers.find((l) => l.id === 'a').blend).toBe('normal')
    await expect(v3(doc, { op: 'blend', layer: 'a', mode: 'sparkle' })).rejects.toThrow()
    const viaLayer = await applyOp(doc, { op: 'layer', name: 'a', set: { blend: 'softLight' } }, {}, undefined, mockE3())
    expect(viaLayer.doc.layers.find((l) => l.id === 'a').blend).toBe('softLight')
  })

  it('style merges one effect, clamped, and undo brings back only that effect', async () => {
    const doc = layered()
    const r1 = await v3(doc, { op: 'style', layer: 'a', effect: 'dropShadow', set: { color: '#FF0000', opacity: 60, size: 999, angle: 120, distance: 8, blend: 'multiply', bogus: 1 } })
    const a1 = r1.doc.layers.find((l) => l.id === 'a')
    expect(a1.styles.dropShadow).toEqual({ color: '#ff0000', opacity: 0.6, size: 250, angle: 120, distance: 8, blend: 'multiply', on: true })
    expect(r1.label).toBe('Drop shadow')
    expect(r1.undo).toEqual({ kind: 'layerStyle', id: 'a', effect: 'dropShadow', before: null })
    const r2 = await v3(r1.doc, { op: 'style', layer: 'a', effect: 'stroke', set: { width: 3, position: 'outside', color: '#ffffff' } })
    const r3 = await v3(r2.doc, { op: 'style', layer: 'a', effect: 'dropShadow', set: { on: false } })
    expect(r3.label).toBe('Drop shadow off')
    expect(r3.doc.layers.find((l) => l.id === 'a').styles.dropShadow).toMatchObject({ on: false, size: 250 })
    // undo the first shadow pill: the shadow goes, the stroke stays
    const back1 = undoPill(r3.doc, { undo: r1.undo }).doc.layers.find((l) => l.id === 'a')
    expect(back1.styles.dropShadow).toBeUndefined()
    expect(back1.styles.stroke).toEqual({ width: 3, position: 'outside', color: '#ffffff', on: true })
    // undo the "off" pill: the shadow is on again as it was
    const back3 = undoPill(r3.doc, { undo: r3.undo }).doc.layers.find((l) => l.id === 'a')
    expect(back3.styles.dropShadow.on).toBe(true)
    const grad = await v3(doc, { op: 'style', layer: 'a', effect: 'gradientOverlay', set: { stops: ['#000000', '#ffffff'] } })
    expect(grad.doc.layers.find((l) => l.id === 'a').styles.gradientOverlay.stops).toHaveLength(2)
    expect(doc.layers.find((l) => l.id === 'a').styles).toBeUndefined()
    await expect(v3(doc, { op: 'style', layer: 'a', effect: 'sparkle', set: {} })).rejects.toThrow()
    await expect(v3(doc, { op: 'style', layer: 'a', effect: 'gradientOverlay', set: { stops: ['#000000'] } })).rejects.toThrow()
  })
})

// ---------- v3: adjustments and filters ----------

describe('v3 adjustment', () => {
  it('adds an adjustment layer on top, masked by the selection; undo removes it', async () => {
    const { doc: start, helpers } = world()
    start.selection = { kind: 'rect', x: 0, y: 0, w: 10, h: 10 }
    const r = await v3(start, { op: 'adjustment', type: 'vibrance', params: { vibrance: 40, saturation: 5000, 'bad key': 1 } }, helpers)
    const top = r.doc.layers.at(-1)
    expect(top).toMatchObject({ kind: 'adjustment', name: 'Vibrance', adjustment: { type: 'vibrance', params: { vibrance: 40, saturation: 5000 } } })
    expect(helpers.masks.get(top.mask)[0]).toBe(255)
    expect(r.label).toBe('Vibrance')
    expect(undoPill(r.doc, { undo: r.undo }).doc.layers).toHaveLength(1)
  })

  it('checks posterize, threshold, gradient map and colour lookup; curves/levels/hsl go through v2', async () => {
    const { doc, helpers } = world()
    const post = await v3(doc, { op: 'adjustment', type: 'posterize', params: { levels: 1 } }, helpers)
    expect(post.doc.layers.at(-1).adjustment.params.levels).toBe(2)
    const thr = await v3(doc, { op: 'adjustment', type: 'threshold', params: {} }, helpers)
    expect(thr.doc.layers.at(-1).adjustment.params.level).toBe(128)
    const map = await v3(doc, { op: 'adjustment', type: 'gradientMap', params: { stops: ['#000000', '#ff8800'] } }, helpers)
    expect(map.doc.layers.at(-1).adjustment.params.stops.map((s) => s.color)).toEqual(['#000000', '#ff8800'])
    const look = await v3(doc, { op: 'adjustment', type: 'colorLookup', params: { look: 'warmFilm' } }, helpers)
    expect(look.label).toBe('Color Lookup')
    await expect(v3(doc, { op: 'adjustment', type: 'colorLookup', params: { look: 'neon' } }, helpers)).rejects.toThrow()
    await expect(v3(doc, { op: 'adjustment', type: 'gradientMap', params: {} }, helpers)).rejects.toThrow()
    await expect(v3(doc, { op: 'adjustment', type: 'sparkle', params: {} }, helpers)).rejects.toThrow()
    const curves = await v3(doc, { op: 'adjustment', type: 'curves', params: { points: [[0, 10], [255, 240]] } }, helpers)
    expect(curves.doc.layers.at(-1).adjustment).toEqual({ type: 'curves', params: { rgb: [[0, 10], [255, 240]] } })
    const levels = await v3(doc, { op: 'adjustment', type: 'levels', params: { black: 10 } }, helpers)
    expect(levels.label).toBe('Levels')
    for (const type of ['colorBalance', 'blackWhite', 'photoFilter', 'selectiveColor', 'channelMixer', 'invert', 'exposure', 'shadowsHighlights', 'brightnessContrast']) {
      const r = await v3(doc, { op: 'adjustment', type, params: { amount: 10 } }, helpers)
      expect(r.doc.layers.at(-1).adjustment.type).toBe(type)
      expect(r.undo.kind).toBe('removeLayer')
    }
  })
})

describe('v3 filter', () => {
  it('runs a new filter on the selected layer inside the selection, with a layerPng undo', async () => {
    const { doc: start, helpers, photoPng } = world()
    start.selection = { kind: 'rect', x: 0, y: 0, w: 10, h: 30 }
    const E3 = mockE3()
    const r = await v3(start, { op: 'filter', name: 'motionBlur', params: { angle: 30, distance: 12 } }, helpers, E3)
    expect(E3.motionBlur).toHaveBeenCalledWith(expect.objectContaining({ width: W, height: H }), { angle: 30, distance: 12 }, expect.any(Uint8Array))
    expect(r.label).toBe('Motion Blur')
    expect(r.undo).toEqual({ kind: 'layerPng', id: 'photo', before: photoPng })
    const out = helpers.store.get(r.doc.layers[0].png)
    expect(px(out, 2, 2)).toEqual([9, 9, 9, 255])
    expect(px(out, 30, 2)).toEqual(SKY)
    expect(undoPill(r.doc, { undo: r.undo }).doc.layers[0].png).toBe(photoPng)
  })

  it('an old filter name still runs on engine2', async () => {
    const { doc, helpers } = world()
    const E3 = mockE3()
    const r = await v3(doc, { op: 'filter', name: 'blur', amount: 20 }, helpers, E3)
    expect(r.label).toBe('Blur 20')
  })
})

// ---------- v3: liquify, warp, perspective ----------

describe('v3 liquify, warp, perspective', () => {
  it('liquify plays every stroke first, then builds one mesh and applies it', async () => {
    const { doc, helpers, photoPng } = world()
    const E3 = mockE3()
    const strokes = [
      { mode: 'forward', points: [[5, 5], [15, 5]], size: 20, pressure: 50 },
      { mode: 'bloat', points: [[20, 20]], size: 9999 },
    ]
    const r = await v3(doc, { op: 'liquify', strokes }, helpers, E3)
    expect(helpers.play.mock.calls.map((c) => c[1])).toEqual(['liquify', 'liquify'])
    expect(helpers.log.lastIndexOf('play:liquify')).toBeLessThan(helpers.log.indexOf('pngFrom'))
    expect(E3.liquifyMesh).toHaveBeenCalledWith(W, H)
    // 'forward' is her word for engine3's push
    expect(E3.liquifyStroke.mock.calls[0][1]).toEqual({ mode: 'push', points: [[5, 5], [15, 5]], size: 20, pressure: 0.5 })
    expect(E3.liquifyStroke.mock.calls[1][1].size).toBe(1000)
    expect(px(helpers.store.get(r.doc.layers[0].png), 0, 0)).toEqual([2, 0, 0, 255])
    expect(r.label).toBe('Liquified (2 strokes)')
    expect(r.undo).toEqual({ kind: 'layerPng', id: 'photo', before: photoPng })
    await expect(v3(doc, { op: 'liquify', strokes: [] }, helpers, E3)).rejects.toThrow()
  })

  it('warp and perspective move the layer; undo restores its pixels and place', async () => {
    const { doc, helpers, photoPng } = world()
    const E3 = mockE3()
    const grid = []
    for (let r = 0; r < 4; r++) for (let c = 0; c < 4; c++) grid.push([c * 10 - 5, r * 8 + 2])
    const w = await v3(doc, { op: 'warp', layer: 'photo', grid }, helpers, E3)
    // drawn into the box of the new shape (x -5..25, y 2..26), the layer moved to that box
    expect(E3.meshWarp).toHaveBeenCalledWith(expect.objectContaining({ width: W }), grid.map(([x, y]) => [x + 5, y - 2]), { width: 30, height: 24 })
    expect([w.doc.layers[0].x, w.doc.layers[0].y]).toEqual([-5, 2])
    expect(helpers.store.get(w.doc.layers[0].png).width).toBe(30)
    expect(w.undo).toEqual({ kind: 'layerPng', id: 'photo', before: photoPng, x: 0, y: 0 })
    expect(w.label).toBe('Warped')
    const back = undoPill(w.doc, { undo: w.undo }).doc.layers[0]
    expect([back.png, back.x, back.y]).toEqual([photoPng, 0, 0])
    await expect(v3(doc, { op: 'warp', layer: 'photo', grid: grid.slice(0, 9) }, helpers, E3)).rejects.toThrow('16 points')

    const p = await v3(doc, { op: 'perspective', layer: 'photo', corners: [[4, 1], [40, 2], [38, 30], [2, 28]] }, helpers, E3)
    expect(helpers.play).toHaveBeenLastCalledWith([[4, 1], [40, 2], [38, 30], [2, 28], [4, 1]], 'perspective')
    expect(E3.perspective.mock.calls[0][2]).toEqual({ width: 38, height: 29 })
    expect([p.doc.layers[0].x, p.doc.layers[0].y]).toEqual([2, 1])
    expect(p.label).toBe('Perspective')
    await expect(v3(doc, { op: 'perspective', corners: [[0, 0]] }, helpers, E3)).rejects.toThrow()
  })
})

// ---------- v3: retouch ----------

describe('v3 retouch brushes', () => {
  it('each brush plays first, then the engine3 brush paints the layer (layerPng undo)', async () => {
    const want = { dodge: 'dodge', burn: 'burn', sponge: 'sponge', smudge: 'smudge', blur_brush: 'blurBrush', sharpen_brush: 'sharpenBrush' }
    for (const [op, fn] of Object.entries(want)) {
      const { doc, helpers, photoPng } = world()
      const E3 = mockE3()
      const r = await v3(doc, { op, points: [[4, 4], [8, 4]], size: 6, strength: 40, range: 'shadows' }, helpers, E3)
      expect(helpers.log.indexOf('play:brush')).toBeLessThan(helpers.log.indexOf('pngFrom'))
      expect(E3[fn]).toHaveBeenCalledTimes(1)
      const spec = E3[fn].mock.calls[0][1]
      expect(spec).toMatchObject({ size: 6, strength: 0.4, points: [[4, 4], [8, 4]] })
      if (op === 'dodge' || op === 'burn') expect(spec.range).toBe('shadows')
      expect(r.undo).toEqual({ kind: 'layerPng', id: 'photo', before: photoPng })
    }
    const { doc, helpers } = world()
    await expect(v3(doc, { op: 'burn', points: [] }, helpers)).rejects.toThrow()
  })

  it('spot_heal and patch (a box, or the selection)', async () => {
    const { doc: start, helpers } = world()
    const E3 = mockE3()
    const heal = await v3(start, { op: 'spot_heal', points: [[3, 3]], size: 5 }, helpers, E3)
    expect(E3.spotHeal.mock.calls[0][1]).toEqual({ points: [[3, 3]], size: 5 })
    expect(heal.label).toBe('Spot healed')
    const boxed = await v3(start, { op: 'patch', x: 14, y: 9, w: 4, h: 4, dx: -10, dy: 0 }, helpers, E3)
    expect(E3.patch.mock.calls[0][2]).toEqual({ dx: -10, dy: 0 })
    const bp = helpers.store.get(boxed.doc.layers[0].png)
    expect(px(bp, 15, 10)).toEqual([105, 105, 105, 255])
    expect(px(bp, 30, 20)).toEqual(GROUND)
    expect(boxed.label).toBe('Patched')
    const sel = { ...start, selection: { kind: 'rect', x: 0, y: 0, w: 3, h: 3 } }
    const viaSel = await v3(sel, { op: 'patch', dx: 5, dy: 5 }, helpers, E3)
    expect(px(helpers.store.get(viaSel.doc.layers[0].png), 1, 1)).toEqual([105, 105, 105, 255])
    expect(helpers.play).toHaveBeenLastCalledWith(expect.any(Array), 'patch')
    await expect(v3(start, { op: 'patch', dx: 5, dy: 5 }, helpers, E3)).rejects.toThrow('select what to patch first')
    await expect(v3(start, { op: 'patch', x: 1, y: 1, w: 2, h: 2 }, helpers, E3)).rejects.toThrow()
  })
})

// ---------- v3: quick and object select ----------

describe('v3 select quick and object', () => {
  it('quick select plays its strokes and sets a mask selection; undo restores the old one', async () => {
    const { doc: start, helpers } = world()
    start.selection = { kind: 'rect', x: 1, y: 1, w: 2, h: 2 }
    const E3 = mockE3()
    const r = await v3(start, { op: 'select', quick: { strokes: [{ points: [[2, 2], [10, 10]], size: 12 }, [[5, 20]]] } }, helpers, E3)
    expect(helpers.play.mock.calls.map((c) => c[1])).toEqual(['select', 'select'])
    expect(E3.quickSelect.mock.calls[0][1]).toEqual([{ points: [[2, 2], [10, 10]], size: 12 }, { points: [[5, 20]], size: 30 }])
    expect(E3.quickSelect.mock.calls[0][2]).toEqual({ add: true, prevMask: null })
    expect(r.label).toBe('Quick selected')
    expect(r.doc.selection.kind).toBe('mask')
    const m = helpers.masks.get(r.doc.selection.png)
    expect([m[0], m[W - 1]]).toEqual([255, 0])
    expect(undoPill(r.doc, { undo: r.undo }).doc.selection).toEqual(start.selection)
    await expect(v3(start, { op: 'select', quick: { strokes: [] } }, helpers, E3)).rejects.toThrow()
    // subtract works on the selection there is
    const sub = await v3(start, { op: 'select', quick: { strokes: [[[2, 2]]], mode: 'subtract' } }, helpers, E3)
    expect(E3.quickSelect.mock.calls.at(-1)[2].add).toBe(false)
    expect(E3.quickSelect.mock.calls.at(-1)[2].prevMask[1 * W + 1]).toBe(255)
    expect(sub.doc.selection.kind).toBe('mask')
    await expect(v3(makeDoc(), { op: 'select', quick: { strokes: [[[2, 2]]], mode: 'subtract' } }, helpers, E3)).rejects.toThrow('no selection')
  })

  it('object select plays the box and selects what the engine finds', async () => {
    const { doc, helpers } = world()
    helpers.composite3 = vi.fn(helpers.composite)
    const E3 = mockE3()
    const r = await v3(doc, { op: 'select', object: { x: 14, y: 9, w: 12, h: 15 } }, helpers, E3)
    expect(helpers.composite3).toHaveBeenCalled()
    expect(E3.objectSelect.mock.calls[0][1]).toEqual({ x: 14, y: 9, w: 12, h: 15 })
    expect(r.label).toBe('Object selected')
    const m = helpers.masks.get(r.doc.selection.png)
    expect([m[16 * W + 20], m[0]]).toEqual([255, 0])
    expect(r.undo).toEqual({ kind: 'selection', before: null })
  })
})

// ---------- v3: guides, artboards, text box ----------

describe('v3 guides and artboards', () => {
  it('a guide is added, clamped, and undone on its own; clearing comes back', async () => {
    const doc = makeDoc()
    const g1 = await applyOp(doc, { op: 'guide', axis: 'h', at: 500 })
    expect(g1.doc.guides).toEqual([{ axis: 'h', at: H }])
    const g2 = await applyOp(g1.doc, { op: 'guide', axis: 'vertical', at: 12.4 })
    expect(g2.doc.guides).toEqual([{ axis: 'h', at: H }, { axis: 'v', at: 12 }])
    expect(g2.label).toBe('Guide at 12')
    expect(undoPill(g2.doc, { undo: g1.undo }).doc.guides).toEqual([{ axis: 'v', at: 12 }])
    await expect(applyOp(g2.doc, { op: 'guide', axis: 'v', at: 12 })).rejects.toThrow()
    await expect(applyOp(doc, { op: 'guide', axis: 'z', at: 1 })).rejects.toThrow()
    const cleared = await applyOp(g2.doc, { op: 'guides_clear' })
    expect(cleared.doc.guides).toEqual([])
    expect(cleared.label).toBe('Guides cleared')
    expect(undoPill(cleared.doc, { undo: cleared.undo }).doc.guides).toEqual(g2.doc.guides)
    await expect(applyOp(doc, { op: 'guides_clear' })).rejects.toThrow()
    expect(doc.guides).toBeUndefined()
  })

  it('an artboard comes from a preset or a size, side by side; undo removes it', async () => {
    const doc = makeDoc()
    const E3 = mockE3()
    const a1 = await applyOp(doc, { op: 'artboard', preset: 'icon-1024' }, {}, undefined, E3)
    expect(a1.doc.artboards).toEqual([{ id: 'artboard-1', name: 'App icon', x: 0, y: 0, w: 1024, h: 1024 }])
    expect(a1.label).toBe('Artboard 1024×1024')
    const a2 = await applyOp(a1.doc, { op: 'artboard', name: 'Banner', w: 1500.6, h: 500 }, {}, undefined, E3)
    expect(a2.doc.artboards[1]).toEqual({ id: 'artboard-2', name: 'Banner', x: 1124, y: 0, w: 1501, h: 500 })
    expect(undoPill(a2.doc, { undo: a1.undo }).doc.artboards.map((a) => a.name)).toEqual(['Banner'])
    await expect(applyOp(doc, { op: 'artboard', preset: 'nope' }, {}, undefined, E3)).rejects.toThrow()
    await expect(applyOp(doc, { op: 'artboard', name: 'x' }, {}, undefined, E3)).rejects.toThrow()
  })

  it('text with a box becomes paragraph text', async () => {
    const r = await applyOp(makeDoc(), { op: 'text', value: 'A longer line that wraps', box: { w: 30, h: 20, align: 'center', lineHeight: 9 } })
    expect(r.doc.layers.at(-1).textBox).toEqual({ w: 30, h: 20, align: 'center', lineHeight: 5, tracking: 0 })
    expect(r.label).toBe('Paragraph text added')
    await expect(applyOp(makeDoc(), { op: 'text', value: 'x', box: { w: 10 } })).rejects.toThrow()
  })
})

// ---------- v3: export, actions, batch ----------

function actionStore(list = []) {
  let items = list
  return { get: vi.fn(() => items), set: vi.fn((next) => (items = next)) }
}

describe('v3 export_as', () => {
  it('asks the view for the files and the core for the folder; its pill cannot be undone', async () => {
    const doc = makeDoc()
    const helpers = {
      exportArtboards: vi.fn(async (d, sizes) => sizes.map((s) => ({ name: s.name, image: 'B64', format: 'png' }))),
      exportMany: vi.fn(async () => 'C:/out'),
    }
    const r = await applyOp(doc, { op: 'export_as', sizes: [{ w: 16, h: 16, name: 'icon/16' }, { w: 32, h: 32 }], format: 'png' }, helpers)
    expect(helpers.exportArtboards).toHaveBeenCalledWith(doc, [{ w: 16, h: 16, name: 'icon-16' }, { w: 32, h: 32, name: '32x32' }], { artboards: false, format: 'png' })
    expect(helpers.exportMany).toHaveBeenCalledWith([{ name: 'icon-16', image: 'B64', format: 'png' }, { name: '32x32', image: 'B64', format: 'png' }])
    expect(r.label).toBe('Exported 2 files')
    expect(r.undo.kind).toBe('none')
    const back = undoPill(r.doc, { undo: r.undo })
    expect(back.doc).toBe(r.doc)
    expect(back.note).toMatch(/cannot be taken back/)
    expect(back.dropAfter).toBe(false)
  })

  it('exports the artboards, and stops when he cancels or there is nothing to export', async () => {
    const doc = { ...makeDoc(), artboards: [{ id: 'artboard-1', name: 'Icon', x: 0, y: 0, w: 10, h: 10 }] }
    const helpers = { exportArtboards: vi.fn(async () => [{ name: 'Icon', image: 'B64' }]), exportMany: vi.fn(async () => 'C:/out') }
    const r = await applyOp(doc, { op: 'export_as', artboards: true, format: 'jpg' }, helpers)
    expect(helpers.exportArtboards).toHaveBeenCalledWith(doc, null, { artboards: true, format: 'jpeg' })
    expect(helpers.exportMany.mock.calls[0][0]).toEqual([{ name: 'Icon', image: 'B64', format: 'jpeg' }])
    expect(r.label).toBe('Exported 1 file')
    helpers.exportMany = vi.fn(async () => null)
    await expect(applyOp(doc, { op: 'export_as', artboards: true }, helpers)).rejects.toThrow('cancelled')
    await expect(applyOp(makeDoc(), { op: 'export_as', artboards: true }, helpers)).rejects.toThrow()
    await expect(applyOp(doc, { op: 'export_as' }, helpers)).rejects.toThrow()
    await expect(applyOp(doc, { op: 'export_as', artboards: true, format: 'gif' }, helpers)).rejects.toThrow()
  })
})

describe('v3 actions', () => {
  it('run_action applies its steps in order as one pill; undo takes them back last to first', async () => {
    const actions = actionStore([{ name: 'Warm Look', steps: [
      { op: 'adjust', values: { warmth: 20 }, relative: true },
      { op: 'guide', axis: 'v', at: 10 },
      { op: 'text', value: 'Hello' },
    ] }])
    const doc = makeDoc()
    const r = await applyOp(doc, { op: 'run_action', name: 'warm look' }, { actions })
    expect(r.label).toBe('Ran Warm Look (3 steps)')
    expect(r.doc.adjust.warmth).toBe(20)
    expect(r.doc.guides).toEqual([{ axis: 'v', at: 10 }])
    expect(r.doc.layers).toHaveLength(2)
    expect(r.undo.kind).toBe('sequence')
    expect(r.undo.undos.map((u) => u.kind)).toEqual(['adjust', 'removeGuide', 'removeLayer'])
    // a later op stays
    const later = await applyOp(r.doc, { op: 'adjust', values: { contrast: 5 }, relative: true })
    const back = undoPill(later.doc, { undo: r.undo })
    expect(back.doc.adjust.warmth).toBe(0)
    expect(back.doc.adjust.contrast).toBe(5)
    expect(back.doc.guides).toEqual([])
    expect(back.doc.layers).toHaveLength(1)
    expect(back.dropAfter).toBe(false)
  })

  it('an unknown action is a plain error; a failing step names itself', async () => {
    const actions = actionStore([{ name: 'Broken', steps: [{ op: 'adjust', values: { warmth: 5 } }, { op: 'teleport' }] }])
    await expect(applyOp(makeDoc(), { op: 'run_action', name: 'Nope' }, { actions })).rejects.toThrow('there is no action called Nope')
    await expect(applyOp(makeDoc(), { op: 'run_action', name: 'Broken' }, { actions })).rejects.toThrow('step 2 of Broken (teleport) failed')
    const loop = actionStore([{ name: 'Loop', steps: [{ op: 'run_action', name: 'Loop' }] }])
    await expect(applyOp(makeDoc(), { op: 'run_action', name: 'Loop' }, { actions: loop })).rejects.toThrow('too deep')
  })

  it('a sequence with a snapshot inside drops the pills after it', async () => {
    const { helpers } = makeKit()
    helpers.actions = actionStore([{ name: 'Crop', steps: [{ op: 'adjust', values: { warmth: 5 }, relative: true }, { op: 'crop', x: 0, y: 0, w: 20, h: 15 }] }])
    const r = await applyOp(makeDoc(), { op: 'run_action', name: 'Crop' }, helpers)
    expect(r.doc.width).toBe(20)
    const back = undoPill(r.doc, { undo: r.undo })
    expect(back.dropAfter).toBe(true)
    expect(back.doc.width).toBe(W)
    expect(back.doc.adjust.warmth).toBe(0)
  })

  it('record_action keeps her last ops of this ask as a named action; its undo takes it back', async () => {
    const actions = actionStore([{ name: 'Old', steps: [{ op: 'guide', axis: 'h', at: 1 }] }])
    const api = {
      photoAsk: vi.fn(async () => ({
        say: 'Recorded.',
        ops: [
          { op: 'adjust', values: { warmth: 10 }, relative: true },
          { op: 'guide', axis: 'v', at: 5 },
          { op: 'text', value: 'Hi' },
          { op: 'record_action', name: 'My Look', from: 'last', count: 2 },
        ],
      })),
    }
    const out = await ask({ doc: makeDoc(), flat: '', message: 'save that as an action' }, api, { actions })
    expect(out.results.map((r) => r.label)).toEqual(['Warmth +10', 'Guide at 5', 'Text added', 'Recorded My Look (2 steps)'])
    expect(actions.set).toHaveBeenCalledTimes(1)
    const saved = actions.get()
    expect(saved.map((a) => a.name)).toEqual(['Old', 'My Look'])
    expect(saved[1].steps).toEqual([{ op: 'guide', axis: 'v', at: 5 }, { op: 'text', value: 'Hi' }])
    const pill = out.results[3]
    expect(pill.undo).toEqual({ kind: 'action', name: 'My Look', before: null })
    const back = undoPill(pill.doc, pill, { actions })
    expect(back.doc).toBe(pill.doc)
    expect(actions.get().map((a) => a.name)).toEqual(['Old'])
    // without the action list the undo says so and changes nothing
    expect(undoPill(pill.doc, pill).note).toMatch(/recording stays/)
  })

  it('re-recording a name replaces it, and undo brings the old one back; steps can be given', async () => {
    const actions = actionStore([{ name: 'Look', steps: [{ op: 'guide', axis: 'h', at: 1 }] }])
    const r = await applyOp(makeDoc(), { op: 'record_action', name: 'look', steps: [{ op: 'adjust', values: { warmth: 3 } }, { op: 'batch', action: 'x', folder: 'y' }] }, { actions })
    expect(actions.get()).toEqual([{ name: 'look', steps: [{ op: 'adjust', values: { warmth: 3 } }] }])
    undoPill(r.doc, r, { actions })
    expect(actions.get()).toEqual([{ name: 'Look', steps: [{ op: 'guide', axis: 'h', at: 1 }] }])
    await expect(applyOp(makeDoc(), { op: 'record_action', name: 'Empty' }, { actions })).rejects.toThrow('no steps')
    await expect(applyOp(makeDoc(), { op: 'record_action', name: '' }, { actions })).rejects.toThrow()
  })

  it('batch runs an action over a folder through the view; not undoable', async () => {
    const actions = actionStore([{ name: 'Warm', steps: [{ op: 'adjust', values: { warmth: 10 }, relative: true }] }])
    const batchFolder = vi.fn(async () => 7)
    const doc = makeDoc()
    const r = await applyOp(doc, { op: 'batch', action: 'warm', folder: 'D:/pics' }, { actions, batchFolder })
    expect(batchFolder).toHaveBeenCalledWith({ action: 'Warm', folder: 'D:/pics', steps: [{ op: 'adjust', values: { warmth: 10 }, relative: true }] })
    expect(r.label).toBe('Batched 7 pictures into PHOTO out')
    expect(r.doc).toBe(doc)
    expect(undoPill(r.doc, r).note).toMatch(/originals were never touched/)
    await expect(applyOp(doc, { op: 'batch', action: 'Cold', folder: 'D:/pics' }, { actions, batchFolder })).rejects.toThrow('there is no action called Cold')
    await expect(applyOp(doc, { op: 'batch', action: 'Warm', folder: ' ' }, { actions, batchFolder })).rejects.toThrow()
  })
})

describe('v3 independent undo across the new ops', () => {
  it('a middle pill of a mixed run undoes on its own', async () => {
    const { doc: start, helpers } = world()
    start.layers.push({ id: 'logo', name: 'Logo', kind: 'image', visible: true, opacity: 1, blend: 'normal', x: 0, y: 0, png: start.layers[0].png, mask: null })
    const E3 = mockE3()
    const ops = [
      { op: 'clip', layer: 'logo', on: true },
      { op: 'style', layer: 'logo', effect: 'stroke', set: { width: 2 } },
      { op: 'blend', layer: 'logo', mode: 'screen' },
      { op: 'guide', axis: 'h', at: 3 },
      { op: 'group', layers: ['logo'] },
    ]
    let doc = start
    const pills = []
    for (const op of ops) {
      const r = await applyOp(doc, op, helpers, undefined, E3)
      pills.push(r)
      doc = r.doc
    }
    const back = undoPill(doc, pills[1]).doc
    const logo = back.layers.find((l) => l.id === 'logo')
    expect(logo.styles.stroke).toBeUndefined()
    expect(logo.clipped).toBe(true)
    expect(logo.blend).toBe('screen')
    expect(back.guides).toHaveLength(1)
    expect(logo.parent).toBe(doc.layers.at(-1).id)
  })
})

// ---------- v3 on the real engine3 ----------

describe('v3 ops on the real engine3', () => {
  const real = (doc, op, helpers) => applyOp(doc, op, helpers)

  it('knows its 27 blend modes, and a group may pass through', async () => {
    expect(engine3.BLEND_MODES).toHaveLength(27)
    const doc = layered()
    const r = await real(doc, { op: 'blend', layer: 'a', mode: 'colorDodge' })
    expect(r.doc.layers.find((l) => l.id === 'a').blend).toBe('colorDodge')
    await expect(real(doc, { op: 'blend', layer: 'a', mode: 'passThrough' })).rejects.toThrow()
    const g = await real(doc, { op: 'group', layers: ['a'] })
    const gid = g.doc.layers.find((l) => l.kind === 'group').id
    const pass = await real(g.doc, { op: 'blend', layer: gid, mode: 'passThrough' })
    expect(pass.doc.layers.find((l) => l.id === gid).blend).toBe('passThrough')
  })

  it('keeps only the params an adjustment knows, with its own label', async () => {
    const { doc, helpers } = world()
    const r = await real(doc, { op: 'adjustment', type: 'vibrance', params: { vibrance: 40, bogus: 1 } }, helpers)
    expect(r.doc.layers.at(-1).adjustment).toEqual({ type: 'vibrance', params: { vibrance: 40 } })
    const cb = await real(doc, { op: 'adjustment', type: 'colorBalance', params: { midtones: [10, 0, -10] } }, helpers)
    expect(cb.label).toBe('Color Balance')
    expect(cb.doc.layers.at(-1).adjustment.params.midtones).toEqual([10, 0, -10])
    const look = await real(doc, { op: 'adjustment', type: 'colorLookup', params: { look: 'portra', amount: 0.5 } }, helpers)
    expect(look.doc.layers.at(-1).adjustment.params).toEqual({ look: 'portra', amount: 0.5 })
    await expect(real(doc, { op: 'adjustment', type: 'colorLookup', params: { look: 'neon' } }, helpers)).rejects.toThrow()
    // the layer it makes renders in compose3
    const out = engine3.compose3([{ ...doc.layers[0], pixels: helpers.store.get(doc.layers[0].png) }, { ...r.doc.layers.at(-1), mask: null }], W, H)
    expect(out.width).toBe(W)
  })

  it('a filter changes only the selection, with a layerPng undo', async () => {
    const { doc: start, helpers, photoPng } = world()
    start.selection = { kind: 'rect', x: 10, y: 5, w: 20, h: 20 }
    const r = await real(start, { op: 'filter', name: 'pixelate', params: { cell: 6, depthMask: [1, 2] } }, helpers)
    expect(r.label).toBe('Pixelate')
    const out = helpers.store.get(r.doc.layers[0].png)
    expect(px(out, 0, 0)).toEqual(SKY)
    expect(px(out, 13, 11)).not.toEqual(SKY)
    expect(r.undo).toEqual({ kind: 'layerPng', id: 'photo', before: photoPng })
    for (const name of ['motionBlur', 'surfaceBlur', 'highPass', 'emboss', 'findEdges', 'addGrain', 'develop']) {
      const f = await real(start, { op: 'filter', name, params: {} }, helpers)
      expect(f.undo.kind).toBe('layerPng')
    }
  })

  it('liquify pushes pixels; warp and perspective with their identity keep the picture in place', async () => {
    const { doc, helpers } = world()
    const liq = await real(doc, { op: 'liquify', strokes: [{ mode: 'forward', points: [[20, 10], [20, 20]], size: 16, pressure: 1 }] }, helpers)
    const lp = helpers.store.get(liq.doc.layers[0].png)
    expect([lp.width, lp.height]).toEqual([W, H])
    expect(lp.data).not.toEqual(helpers.store.get(doc.layers[0].png).data)
    const warp = await real(doc, { op: 'warp', layer: 'photo', grid: engine3.defaultWarpGrid(W, H) }, helpers)
    const wp = helpers.store.get(warp.doc.layers[0].png)
    expect([wp.width, wp.height, warp.doc.layers[0].x, warp.doc.layers[0].y]).toEqual([W, H, 0, 0])
    expect(px(wp, 20, 16)).toEqual(RED)
    const persp = await real(doc, { op: 'perspective', layer: 'photo', corners: [[0, 0], [W, 0], [W, H], [0, H]] }, helpers)
    const pp = helpers.store.get(persp.doc.layers[0].png)
    expect([pp.width, pp.height]).toEqual([W, H])
    expect(px(pp, 20, 16)).toEqual(RED)
  })

  it('dodge lightens and burn darkens along the stroke; spot heal and patch mend', async () => {
    const { doc, helpers } = world()
    const d = await real(doc, { op: 'dodge', points: [[5, 20], [10, 20]], size: 8, strength: 80, range: 'midtones' }, helpers)
    expect(px(helpers.store.get(d.doc.layers[0].png), 7, 20)[1]).toBeGreaterThan(GROUND[1])
    const b = await real(doc, { op: 'burn', points: [[5, 20], [10, 20]], size: 8, strength: 80 }, helpers)
    expect(px(helpers.store.get(b.doc.layers[0].png), 7, 20)[1]).toBeLessThan(GROUND[1])
    for (const op of ['sponge', 'smudge', 'blur_brush', 'sharpen_brush']) {
      const r = await real(doc, { op, points: [[12, 10], [28, 10]], size: 6, strength: 60 }, helpers)
      expect(r.undo.kind).toBe('layerPng')
    }
    const heal = await real(doc, { op: 'spot_heal', points: [[30, 20]], size: 4 }, helpers)
    expect(heal.undo.kind).toBe('layerPng')
    const patch = await real(doc, { op: 'patch', x: 14, y: 20, w: 12, h: 4, dx: 0, dy: 6 }, helpers)
    expect(px(helpers.store.get(patch.doc.layers[0].png), 20, 21)).not.toEqual(RED)
  })

  it('quick and object select find the red box', async () => {
    const { doc, helpers } = world()
    const q = await real(doc, { op: 'select', quick: { strokes: [{ points: [[18, 14], [22, 18]], size: 4 }] } }, helpers)
    const qm = helpers.masks.get(q.doc.selection.png)
    expect([qm[16 * W + 20], qm[2 * W + 2]]).toEqual([255, 0])
    const o = await real(doc, { op: 'select', object: { x: 11, y: 6, w: 18, h: 21 } }, helpers)
    const om = helpers.masks.get(o.doc.selection.png)
    expect(om[16 * W + 20]).toBeGreaterThan(200)
    expect(om[1 * W + 1]).toBe(0)
  })

  it('artboard presets come from engine3', async () => {
    const r = await real(makeDoc(), { op: 'artboard', preset: 'icon-1024' })
    expect(r.doc.artboards[0]).toMatchObject({ name: 'App icon 1024', w: 1024, h: 1024 })
    const byName = await real(makeDoc(), { op: 'artboard', preset: 'full hd 1920 x 1080' })
    expect(byName.doc.artboards[0]).toMatchObject({ w: 1920, h: 1080 })
  })
})

// ---------- ask ----------

describe('ask', () => {
  it('sends the doc without pixels, applies ops in order and skips a bad one with a note', async () => {
    const { doc, helpers } = world()
    const api = {
      photoAsk: vi.fn(async () => ({
        say: 'Here you go.',
        ops: [
          { op: 'adjust', values: { warmth: 18 }, relative: true },
          { op: 'generate_layer', prompt: 'a balloon' },
          { op: 'replace_sky' },
          { op: 'adjust', values: { warmth: 5 }, relative: true },
          { op: 'teleport' },
        ],
      })),
      photoGenerate: vi.fn(async () => {
        throw new Error('the image service is down')
      }),
    }
    const { generate, ...noGenerate } = helpers
    const out = await ask({ doc, flat: 'FLAT0', message: 'warmer with a sunset' }, api, noGenerate)
    const sent = api.photoAsk.mock.calls[0][0]
    expect(sent.flat).toBe('FLAT0')
    expect(sent.doc.layers[0]).not.toHaveProperty('png')
    expect(api.photoGenerate).toHaveBeenCalledTimes(1)
    expect(api.photoGenerate.mock.calls[0][0].kind).toBe('generate_layer')
    expect(out.results.map((r) => r.label)).toEqual(['Warmth +18', 'Sky replaced', 'Warmth +5'])
    expect(out.results[2].doc.adjust.warmth).toBe(23)
    expect(out.results[2].doc.layers.map((l) => l.name)).toEqual(['Photo', 'Companion - sky'])
    expect(out.say).toBe('Here you go. (I couldn\'t draw that layer: the image service is down) (I couldn\'t do "teleport": I don\'t know "teleport")')
  })

  it('copes with a reply without ops', async () => {
    const out = await ask({ doc: makeDoc(), flat: '', message: 'hi' }, { photoAsk: async () => ({ say: 'Hello!' }) })
    expect(out).toEqual({ say: 'Hello!', results: [] })
  })
})

// ---------- her cursor's path ----------

describe('cursor path', () => {
  it('reads points in both shapes and measures the path', () => {
    expect(normPoints([[1, 2], { x: 3, y: 4 }, ['a', 1], null])).toEqual([{ x: 1, y: 2 }, { x: 3, y: 4 }])
    expect(pathLength([[0, 0], [3, 4], [3, 10]])).toBe(11)
    expect(pathLength([[5, 5]])).toBe(0)
  })

  it('pointAt walks by distance', () => {
    const path = [[0, 0], [10, 0], [10, 10]]
    expect(pointAt(path, 0)).toEqual({ x: 0, y: 0 })
    expect(pointAt(path, 0.25)).toEqual({ x: 5, y: 0 })
    expect(pointAt(path, 0.75)).toEqual({ x: 10, y: 5 })
    expect(pointAt(path, 2)).toEqual({ x: 10, y: 10 })
    expect(pointAt([[4, 4]], 0.5)).toEqual({ x: 4, y: 4 })
    expect(pointAt([], 0.5)).toBe(null)
  })

  it('takes 600 ms to 2 s by length, and eases at both ends', () => {
    expect(durationFor(0)).toBe(MIN_MS)
    expect(durationFor(400)).toBe(1200)
    expect(durationFor(1e6)).toBe(MAX_MS)
    expect(ease(0)).toBe(0)
    expect(ease(0.5)).toBe(0.5)
    expect(ease(1)).toBe(1)
    expect(ease(0.1)).toBeLessThan(0.1)
  })
})
