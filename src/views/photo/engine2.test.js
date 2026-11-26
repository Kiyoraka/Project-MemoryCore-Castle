import { describe, expect, it } from 'vitest'
import { makePixels } from './engine.js'
import {
  addNoise,
  applyCurves,
  applyHsl,
  applyLevels,
  applyMask,
  colorRange,
  combineMasks,
  compose2,
  contentAwareFill,
  curvesLut,
  drawGradient,
  drawShape,
  gaussianBlur,
  grabCut,
  heal,
  histogram,
  invertMask,
  matrixFor,
  median,
  refineMask,
  skyMask,
  transformPixels,
  unsharp,
  vignette,
} from './engine2.js'

const px = (p, x, y) => {
  const i = (y * p.width + x) * 4
  return Array.from(p.data.slice(i, i + 4))
}
const setPx = (p, x, y, rgba) => p.data.set(rgba, (y * p.width + x) * 4)
const area = (m) => m.reduce((a, v) => a + (v > 127 ? 1 : 0), 0)

// a picture where every pixel differs, for exactness checks
function ramp(w, h) {
  const p = makePixels(w, h)
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) setPx(p, x, y, [x * 20, y * 30, (x + y) * 10, 255])
  return p
}

describe('tone and colour', () => {
  it('curvesLut is the identity for the diagonal and monotonic for an S', () => {
    const id = curvesLut([[0, 0], [255, 255]])
    for (let i = 0; i < 256; i++) expect(id[i]).toBe(i)
    const s = curvesLut([[0, 0], [64, 40], [192, 215], [255, 255]])
    for (let i = 1; i < 256; i++) expect(s[i]).toBeGreaterThanOrEqual(s[i - 1])
    expect(s[64]).toBe(40)
    expect(s[192]).toBe(215)
    expect(s[32]).toBeLessThan(32)
    expect(s[224]).toBeGreaterThan(224)
  })

  it('applyCurves runs a channel curve and respects a mask', () => {
    const p = makePixels(2, 1, [100, 100, 100, 255])
    const out = applyCurves(p, { r: [[0, 0], [100, 200], [255, 255]] }, Uint8Array.from([255, 0]))
    expect(px(out, 0, 0)).toEqual([200, 100, 100, 255])
    expect(px(out, 1, 0)).toEqual([100, 100, 100, 255])
    expect(px(p, 0, 0)).toEqual([100, 100, 100, 255])
  })

  it('levels: identity by default, gamma lifts the midtones, black/white stretch', () => {
    const p = ramp(4, 3)
    expect(Array.from(applyLevels(p, {}).data)).toEqual(Array.from(p.data))
    const mid = makePixels(1, 1, [128, 128, 128, 255])
    expect(px(applyLevels(mid, { gamma: 2 }), 0, 0)[0]).toBeGreaterThan(170)
    expect(px(applyLevels(mid, { gamma: 0.5 }), 0, 0)[0]).toBeLessThan(80)
    expect(px(applyLevels(mid, { black: 128 }), 0, 0)[0]).toBe(0)
    expect(px(applyLevels(mid, { white: 128 }), 0, 0)[0]).toBe(255)
  })

  it('hsl: rotating the reds turns pure red toward yellow and leaves blue alone', () => {
    const p = makePixels(2, 1, [255, 0, 0, 255])
    setPx(p, 1, 0, [0, 0, 255, 255])
    const out = applyHsl(p, { ranges: { reds: { hue: 30, sat: 0, light: 0 } } })
    const [r, g, b] = px(out, 0, 0)
    expect(r).toBe(255)
    expect(g).toBeGreaterThan(100)
    expect(g).toBeLessThan(160)
    expect(b).toBe(0)
    expect(px(out, 1, 0)).toEqual([0, 0, 255, 255])
    const grey = applyHsl(makePixels(1, 1, [128, 128, 128, 255]), { ranges: { all: { sat: 0, hue: 0, light: 50 } } })
    expect(px(grey, 0, 0)[0]).toBeGreaterThan(180)
  })

  it('histogram counts channels and luma, skipping transparent pixels', () => {
    const p = makePixels(3, 1, [10, 20, 30, 255])
    setPx(p, 2, 0, [0, 0, 0, 0])
    const h = histogram(p)
    expect(h.r[10]).toBe(2)
    expect(h.g[20]).toBe(2)
    expect(h.b[30]).toBe(2)
    expect(h.luma.reduce((a, v) => a + v, 0)).toBe(2)
  })
})

describe('masks and layers', () => {
  it('applyMask, invertMask and combineMasks', () => {
    const p = makePixels(2, 1, [1, 2, 3, 200])
    const m = Uint8Array.from([255, 0])
    const out = applyMask(p, m)
    expect(px(out, 0, 0)[3]).toBe(200)
    expect(px(out, 1, 0)[3]).toBe(0)
    expect(Array.from(invertMask(m))).toEqual([0, 255])
    const a = Uint8Array.from([255, 255, 0, 100])
    const b = Uint8Array.from([255, 0, 255, 50])
    expect(Array.from(combineMasks(a, b, 'add'))).toEqual([255, 255, 255, 100])
    expect(Array.from(combineMasks(a, b, 'subtract'))).toEqual([0, 255, 0, 100])
    expect(Array.from(combineMasks(a, b, 'intersect'))).toEqual([255, 0, 0, 50])
    expect(Array.from(combineMasks(a, b, 'xor'))).toEqual([0, 255, 255, 50])
  })

  it('compose2: an adjustment layer changes only what is below it, within its mask', () => {
    const W = 2
    const H = 1
    const base = makePixels(W, H, [100, 100, 100, 255])
    const top = makePixels(W, H)
    setPx(top, 1, 0, [10, 20, 30, 255]) // covers the right pixel, above the adjustment
    const invert = { type: 'curves', params: { rgb: [[0, 255], [255, 0]] } }
    const layers = [
      { kind: 'image', pixels: base, opacity: 1, blend: 'normal', visible: true },
      { kind: 'adjustment', opacity: 1, blend: 'normal', visible: true, adjustment: invert },
      { kind: 'image', pixels: top, opacity: 1, blend: 'normal', visible: true },
    ]
    const out = compose2(layers, W, H)
    expect(px(out, 0, 0)).toEqual([155, 155, 155, 255])
    expect(px(out, 1, 0)).toEqual([10, 20, 30, 255])
    // masked: only the left pixel is adjusted
    const masked = compose2(
      [layers[0], { ...layers[1], mask: Uint8Array.from([0, 255]) }],
      W,
      H,
    )
    expect(px(masked, 0, 0)).toEqual([100, 100, 100, 255])
    expect(px(masked, 1, 0)).toEqual([155, 155, 155, 255])
    // half opacity mixes halfway; a hidden layer does nothing
    const half = compose2([layers[0], { ...layers[1], opacity: 0.5 }], W, H)
    expect(Math.abs(px(half, 0, 0)[0] - 128)).toBeLessThanOrEqual(1)
    const hidden = compose2([layers[0], { ...layers[1], visible: false }], W, H)
    expect(px(hidden, 0, 0)).toEqual([100, 100, 100, 255])
  })

  it('compose2: an image layer blends through its mask; adjust uses engine.js', () => {
    const base = makePixels(2, 1, [0, 0, 0, 255])
    const red = makePixels(2, 1, [255, 0, 0, 255])
    const out = compose2(
      [
        { kind: 'image', pixels: base, opacity: 1, blend: 'normal' },
        { kind: 'text', pixels: red, opacity: 1, blend: 'normal', mask: Uint8Array.from([255, 0]) },
        { kind: 'adjustment', opacity: 1, adjustment: { type: 'adjust', params: { brightness: 50 } } },
      ],
      2,
      1,
    )
    expect(px(out, 0, 0)[0]).toBe(255)
    expect(px(out, 1, 0)[0]).toBe(64)
  })
})

describe('geometry', () => {
  it('identity transform copies exactly', () => {
    const p = ramp(5, 4)
    const out = transformPixels(p, matrixFor({}), 5, 4)
    expect(Array.from(out.data)).toEqual(Array.from(p.data))
  })

  it('a 90-degree turn moves (x, y) to (h - 1 - y, x)', () => {
    const p = ramp(3, 2)
    const out = transformPixels(p, matrixFor({ rotate: 90, dx: 2 }), 2, 3)
    for (let y = 0; y < 2; y++) for (let x = 0; x < 3; x++) expect(px(out, 2 - 1 - y, x)).toEqual(px(p, x, y))
  })

  it('scale 2 about the origin doubles the picture', () => {
    const p = makePixels(2, 2, [50, 60, 70, 255])
    const out = transformPixels(p, matrixFor({ scale: 2 }), 4, 4)
    expect(px(out, 2, 2)).toEqual([50, 60, 70, 255])
  })
})

describe('drawing', () => {
  it('a linear gradient hits its end colours', () => {
    const p = makePixels(20, 2, [0, 0, 0, 255])
    const out = drawGradient(p, {
      kind: 'linear',
      from: [0, 0],
      to: [20, 0],
      stops: [
        { at: 0, color: '#ff0000', alpha: 1 },
        { at: 1, color: '#0000ff', alpha: 1 },
      ],
    })
    const left = px(out, 0, 0)
    const right = px(out, 19, 0)
    expect(left[0]).toBeGreaterThan(240)
    expect(left[2]).toBeLessThan(15)
    expect(right[2]).toBeGreaterThan(240)
    expect(right[0]).toBeLessThan(15)
    const radial = drawGradient(p, { kind: 'radial', from: [10, 1], to: [20, 1], stops: [{ at: 0, color: '#ffffff' }, { at: 1, color: '#000000' }] })
    expect(px(radial, 10, 1)[0]).toBeGreaterThan(px(radial, 2, 1)[0])
  })

  it('shapes fill inside and not outside, edges anti-aliased', () => {
    const p = makePixels(20, 20, [0, 0, 0, 255])
    const rect = drawShape(p, { kind: 'rect', x: 5, y: 5, w: 10, h: 10, fill: '#ffffff', stroke: null })
    expect(px(rect, 10, 10)[0]).toBe(255)
    expect(px(rect, 2, 2)[0]).toBe(0)
    expect(px(rect, 15, 10)[0]).toBe(0)
    const ell = drawShape(p, { kind: 'ellipse', x: 2, y: 2, w: 16, h: 16, fill: '#ffffff' })
    expect(px(ell, 10, 10)[0]).toBe(255)
    expect(px(ell, 0, 0)[0]).toBe(0)
    let partial = 0
    for (let i = 0; i < ell.data.length; i += 4) if (ell.data[i] > 0 && ell.data[i] < 255) partial++
    expect(partial).toBeGreaterThan(8) // a soft rim, not a staircase
    const line = drawShape(p, { kind: 'line', points: [[0, 10], [20, 10]], stroke: '#ffffff', width: 2 })
    expect(px(line, 10, 9)[0]).toBe(255)
    expect(px(line, 10, 5)[0]).toBe(0)
    const tri = drawShape(p, { kind: 'polygon', points: [[0, 0], [20, 0], [0, 20]], fill: '#ff0000', stroke: '#00ff00', width: 1 })
    expect(px(tri, 4, 4)[0]).toBe(255)
    expect(px(tri, 16, 16)[0]).toBe(0)
  })
})

describe('filters', () => {
  it('blur keeps a flat picture flat and softens an edge', () => {
    const flat = makePixels(8, 8, [90, 120, 150, 255])
    expect(Array.from(gaussianBlur(flat, 2).data)).toEqual(Array.from(flat.data))
    const edge = makePixels(10, 1, [0, 0, 0, 255])
    for (let x = 5; x < 10; x++) setPx(edge, x, 0, [255, 255, 255, 255])
    const b = gaussianBlur(edge, 1.5)
    expect(px(b, 4, 0)[0]).toBeGreaterThan(20)
    expect(px(b, 5, 0)[0]).toBeLessThan(235)
    expect(px(b, 0, 0)[0]).toBeLessThan(5)
  })

  it('unsharp raises contrast across an edge and leaves flat areas', () => {
    const edge = makePixels(10, 1, [100, 100, 100, 255])
    for (let x = 5; x < 10; x++) setPx(edge, x, 0, [150, 150, 150, 255])
    const s = unsharp(edge, { amount: 100, radius: 1, threshold: 0 })
    expect(px(s, 4, 0)[0]).toBeLessThan(100)
    expect(px(s, 5, 0)[0]).toBeGreaterThan(150)
    expect(px(s, 0, 0)[0]).toBe(100)
  })

  it('noise is the same with the same seed and different with another', () => {
    const p = makePixels(6, 6, [128, 128, 128, 255])
    const a = addNoise(p, 20, 7)
    const b = addNoise(p, 20, 7)
    const c = addNoise(p, 20, 8)
    expect(Array.from(a.data)).toEqual(Array.from(b.data))
    expect(Array.from(a.data)).not.toEqual(Array.from(c.data))
    expect(Array.from(a.data)).not.toEqual(Array.from(p.data))
  })

  it('vignette darkens the corners more than the centre', () => {
    const p = makePixels(21, 21, [200, 200, 200, 255])
    const v = vignette(p, { amount: 60, size: 30 })
    expect(px(v, 10, 10)[0]).toBe(200)
    expect(px(v, 0, 0)[0]).toBeLessThan(120)
  })

  it('median removes a single salt pixel', () => {
    const p = makePixels(5, 5, [40, 40, 40, 255])
    setPx(p, 2, 2, [255, 255, 255, 255])
    const m = median(p, 1)
    expect(px(m, 2, 2)).toEqual([40, 40, 40, 255])
  })

  it('a mask limits where a filter applies', () => {
    const p = makePixels(4, 1, [128, 128, 128, 255])
    const m = Uint8Array.from([255, 255, 0, 0])
    const n = addNoise(p, 50, 3, m)
    expect(px(n, 3, 0)).toEqual([128, 128, 128, 255])
  })
})

describe('retouch', () => {
  it('heal changes only near the points', () => {
    const p = makePixels(40, 20, [120, 120, 120, 255])
    for (let x = 0; x < 40; x++) for (let y = 0; y < 20; y++) setPx(p, x, y, [120 + (x % 3), 120, 120, 255])
    setPx(p, 30, 10, [0, 0, 0, 255]) // the blemish
    const out = heal(p, { source: [10, 10], points: [[30, 10]], size: 6 })
    expect(px(out, 30, 10)[0]).toBeGreaterThan(80)
    for (let y = 0; y < 20; y++) {
      for (let x = 0; x < 40; x++) {
        if (Math.hypot(x + 0.5 - 30, y + 0.5 - 10) <= 4) continue
        expect(px(out, x, y)).toEqual(px(p, x, y))
      }
    }
  })

  it('content-aware fill restores vertical stripes inside a hole', () => {
    const W = 40
    const H = 30
    const colours = [
      [220, 40, 40, 255],
      [40, 60, 220, 255],
    ]
    const stripe = (x) => colours[Math.floor(x / 4) % 2]
    const p = makePixels(W, H)
    for (let y = 0; y < H; y++) for (let x = 0; x < W; x++) setPx(p, x, y, stripe(x))
    const hole = new Uint8Array(W * H)
    for (let y = 10; y < 20; y++) {
      for (let x = 14; x < 26; x++) {
        hole[y * W + x] = 255
        setPx(p, x, y, [0, 255, 0, 255]) // something to remove
      }
    }
    const out = contentAwareFill(p, hole, { iterations: 5, patch: 7, seed: 3 })
    let good = 0
    let total = 0
    for (let y = 10; y < 20; y++) {
      for (let x = 14; x < 26; x++) {
        total++
        const [r, g, b] = px(out, x, y)
        const want = stripe(x)
        if (Math.abs(r - want[0]) + Math.abs(g - want[1]) + Math.abs(b - want[2]) < 60) good++
      }
    }
    expect(good / total).toBeGreaterThan(0.8)
    // outside the hole nothing moved
    expect(px(out, 0, 0)).toEqual(stripe(0))
    expect(px(out, 39, 29)).toEqual(stripe(39))
  })
})

describe('selection', () => {
  it('refineMask grows, shrinks and feathers', () => {
    const W = 20
    const H = 20
    const m = new Uint8Array(W * H)
    for (let y = 5; y < 15; y++) for (let x = 5; x < 15; x++) m[y * W + x] = 255
    expect(area(refineMask(m, W, H, { shift: 2 }))).toBeGreaterThan(area(m))
    expect(area(refineMask(m, W, H, { shift: -2 }))).toBeLessThan(area(m))
    const f = refineMask(m, W, H, { feather: 2 })
    expect(f[10 * W + 5]).toBeGreaterThan(0)
    expect(f[10 * W + 5]).toBeLessThan(255)
    expect(f[10 * W + 10]).toBeGreaterThan(240)
    const speck = new Uint8Array(m)
    speck[1 * W + 1] = 255
    expect(refineMask(speck, W, H, { smooth: 3 })[1 * W + 1]).toBe(0)
  })

  it('colorRange selects the given colour softly', () => {
    const p = makePixels(3, 1, [255, 0, 0, 255])
    setPx(p, 1, 0, [235, 10, 10, 255])
    setPx(p, 2, 0, [0, 0, 255, 255])
    const m = colorRange(p, '#ff0000', 40)
    expect(m[0]).toBe(255)
    expect(m[1]).toBeGreaterThan(100)
    expect(m[2]).toBe(0)
  })

  it('grabCut finds a red square on grey', () => {
    const W = 60
    const H = 40
    const p = makePixels(W, H, [128, 128, 128, 255])
    // a little grain so the models are not degenerate
    const noisy = addNoise(p, 3, 5)
    for (let y = 12; y < 28; y++) for (let x = 22; x < 38; x++) setPx(noisy, x, y, [210, 30, 30, 255])
    const m = grabCut(noisy, { x: 15, y: 6, w: 30, h: 28 })
    let inter = 0
    let union = 0
    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        const truth = x >= 22 && x < 38 && y >= 12 && y < 28
        const sel = m[y * W + x] > 127
        if (truth && sel) inter++
        if (truth || sel) union++
      }
    }
    expect(inter / union).toBeGreaterThan(0.85)
  })

  it('grabCut works through its downscale too', () => {
    const W = 200
    const H = 120
    const p = addNoise(makePixels(W, H, [120, 130, 120, 255]), 3, 9)
    for (let y = 30; y < 90; y++) for (let x = 70; x < 130; x++) setPx(p, x, y, [30, 40, 200, 255])
    const m = grabCut(p, { x: 50, y: 15, w: 100, h: 90 }, { maxSide: 80 })
    let inter = 0
    let union = 0
    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        const truth = x >= 70 && x < 130 && y >= 30 && y < 90
        const sel = m[y * W + x] > 127
        if (truth && sel) inter++
        if (truth || sel) union++
      }
    }
    expect(inter / union).toBeGreaterThan(0.85)
  })

  it('skyMask selects a blue top half over a green bottom half', () => {
    const W = 40
    const H = 30
    const p = makePixels(W, H)
    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        if (y < 15) setPx(p, x, y, [90 + y * 2, 150 + y, 230, 255])
        else setPx(p, x, y, [40, 140, 50, 255])
      }
    }
    const m = skyMask(p, { tolerance: 40 })
    let topSel = 0
    let bottomSel = 0
    for (let y = 0; y < H; y++) {
      for (let x = 0; x < W; x++) {
        if (m[y * W + x] <= 127) continue
        if (y < 15) topSel++
        else bottomSel++
      }
    }
    expect(topSel / (W * 15)).toBeGreaterThan(0.9)
    expect(bottomSel / (W * 15)).toBeLessThan(0.05)
  })
})
