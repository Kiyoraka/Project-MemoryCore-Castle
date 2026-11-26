import { describe, expect, it } from 'vitest'
import { makePixels } from './engine.js'
import { compose2 } from './engine2.js'
import {
  ADJUSTMENTS,
  BLEND_MODES,
  FILTERS,
  LOOKS,
  STYLE_DEFAULTS,
  addGrain,
  applyAdjustment3,
  applyFilter,
  applyHomography,
  applyMesh,
  artboardPresets,
  blackWhite,
  blend3,
  blendRgb,
  blurBrush,
  brightnessContrast,
  burn,
  channelMixer,
  colorBalance,
  colorLookup,
  compose3,
  defaultWarpGrid,
  develop,
  dodge,
  emboss,
  exposure,
  findEdges,
  fitInto,
  gradientMap,
  hashNoise,
  highPass,
  homography,
  invert,
  lensBlur,
  liquifyMesh,
  liquifyStroke,
  meshWarp,
  motionBlur,
  objectSelect,
  oilPaint,
  patch,
  perspective,
  photoFilter,
  pixelate,
  posterize,
  quickSelect,
  radialBlur,
  renderStyles,
  selectiveColor,
  shadowsHighlights,
  sharpenBrush,
  smudge,
  snapBox,
  snapTo,
  spotHeal,
  sponge,
  surfaceBlur,
  threshold,
  vibrance,
} from './engine3.js'

const px = (p, x, y) => Array.from(p.data.slice((y * p.width + x) * 4, (y * p.width + x) * 4 + 4))
const luma = ([r, g, b]) => 0.299 * r + 0.587 * g + 0.114 * b
const near = (a, b, tol = 2) => a.every((v, i) => Math.abs(v - b[i]) <= tol)

// A transparent w x h picture with an opaque square of colour rgba.
function square(w, h, x0, y0, s, rgba = [255, 0, 0, 255]) {
  const p = makePixels(w, h)
  for (let y = y0; y < y0 + s; y++) for (let x = x0; x < x0 + s; x++) p.data.set(rgba, (y * w + x) * 4)
  return p
}

function ramp(w, h) {
  const p = makePixels(w, h)
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) p.data.set([Math.round((x / (w - 1)) * 255), Math.round((y / (h - 1)) * 255), 128, 255], (y * w + x) * 4)
  return p
}

function colourful(w = 12, h = 12) {
  const p = makePixels(w, h)
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) p.data.set([(x * 23) % 256, (y * 37) % 256, ((x + y) * 19) % 256, 255], (y * w + x) * 4)
  return p
}

const variance = (p, x0, y0, x1, y1) => {
  const vals = []
  for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) vals.push(luma(px(p, x, y)))
  const m = vals.reduce((a, b) => a + b, 0) / vals.length
  return vals.reduce((a, b) => a + (b - m) ** 2, 0) / vals.length
}

describe('blend modes', () => {
  it('lists the 27 Photoshop modes once each', () => {
    expect(BLEND_MODES).toHaveLength(27)
    expect(new Set(BLEND_MODES).size).toBe(27)
  })

  it('computes the separable formulas', () => {
    const g = [128, 128, 128, 255]
    expect(blend3('normal', [10, 20, 30, 255], g)).toEqual([10, 20, 30, 255])
    expect(blend3('multiply', [255, 0, 0, 255], g)).toEqual([128, 0, 0, 255])
    expect(blend3('screen', g, g)[0]).toBe(192)
    expect(blend3('difference', [200, 50, 0, 255], [50, 50, 50, 255]).slice(0, 3)).toEqual([150, 0, 50])
    expect(blend3('subtract', [50, 50, 50, 255], [200, 20, 50, 255]).slice(0, 3)).toEqual([150, 0, 0])
    expect(blend3('linearDodge', [200, 0, 0, 255], [100, 0, 0, 255])[0]).toBe(255)
    expect(blend3('linearBurn', [200, 0, 0, 255], [100, 0, 0, 255])[0]).toBe(45)
    expect(blend3('hardMix', [200, 50, 0, 255], [100, 100, 0, 255]).slice(0, 3)).toEqual([255, 0, 0])
    expect(blend3('divide', [128, 128, 128, 255], [64, 64, 64, 255])[0]).toBe(128)
    expect(Math.abs(blend3('colorDodge', [128, 128, 128, 255], [64, 64, 64, 255])[0] - 128)).toBeLessThanOrEqual(1)
    expect(blend3('colorBurn', [255, 255, 255, 255], [64, 64, 64, 255])[0]).toBe(64)
    expect(blend3('exclusion', g, g)[0]).toBe(127)
    // overlay is hard light with the layers swapped
    expect(blend3('overlay', [200, 0, 0, 255], [60, 0, 0, 255])[0]).toBe(blend3('hardLight', [60, 0, 0, 255], [200, 0, 0, 255])[0])
    expect(blend3('pinLight', [10, 0, 0, 255], [100, 0, 0, 255])[0]).toBe(20)
  })

  it('computes the non-separable modes', () => {
    const red = [255, 0, 0, 255]
    const grey = [128, 128, 128, 255]
    const col = blend3('color', red, grey)
    expect(col[0]).toBeGreaterThan(col[1])
    expect(Math.abs(0.3 * col[0] + 0.59 * col[1] + 0.11 * col[2] - 128)).toBeLessThan(2)
    const lum = blend3('luminosity', [200, 200, 200, 255], [255, 0, 0, 255])
    expect(Math.abs(0.3 * lum[0] + 0.59 * lum[1] + 0.11 * lum[2] - 200)).toBeLessThan(2)
    expect(lum[0]).toBeGreaterThan(lum[1])
    expect(blend3('darkerColor', [10, 200, 10, 255], [100, 100, 100, 255]).slice(0, 3)).toEqual([100, 100, 100])
    expect(blend3('lighterColor', [10, 200, 10, 255], [100, 100, 100, 255]).slice(0, 3)).toEqual([10, 200, 10])
    const hue = blendRgb('hue', [0, 0, 1], [1, 0.5, 0.5])
    expect(hue[2]).toBeGreaterThan(hue[0])
    const sat = blendRgb('saturation', [0.5, 0.5, 0.5], [1, 0, 0])
    expect(sat[0]).toBeCloseTo(sat[1], 5)
  })

  it('dissolves by noise and stays in range for every mode', () => {
    const half = [255, 0, 0, 128]
    const dst = [0, 0, 255, 255]
    expect(blend3('dissolve', half, dst, 1, 0.1)).toEqual([255, 0, 0, 255])
    expect(blend3('dissolve', half, dst, 1, 0.9)).toEqual(dst)
    for (const mode of BLEND_MODES) {
      const r = blend3(mode, [200, 90, 30, 180], [40, 160, 220, 200], 0.7, hashNoise(5))
      for (const v of r) expect(v >= 0 && v <= 255 && Number.isFinite(v)).toBe(true)
    }
    expect(blend3('normal', [255, 0, 0, 255], [0, 0, 0, 0], 0.5)[3]).toBe(128)
    expect(hashNoise(3, 1)).toBe(hashNoise(3, 1))
    expect(hashNoise(3, 1)).not.toBe(hashNoise(4, 1))
  })
})

describe('layer styles', () => {
  const sq = square(30, 30, 10, 10, 10)

  it('drops a shadow away from the light', () => {
    const { below, above, belowList } = renderStyles(sq, { dropShadow: { distance: 4, size: 0, opacity: 1 } })
    expect(belowList[0].name).toBe('dropShadow')
    // light at 120 degrees (upper left): the shadow falls down and to the right
    expect(px(below, 21, 22)[3]).toBe(255)
    expect(px(below, 9, 9)[3]).toBe(0)
    expect(px(above, 15, 15)[3]).toBe(0)
  })

  it('strokes outside, inside and on the centre of the edge', () => {
    const out = renderStyles(sq, { stroke: { width: 2, color: '#0000ff' } }).above
    expect(px(out, 9, 15)).toEqual([0, 0, 255, 255])
    expect(px(out, 8, 15)[3]).toBe(255)
    expect(px(out, 6, 15)[3]).toBe(0)
    expect(px(out, 12, 15)[3]).toBe(0)
    const inside = renderStyles(sq, { stroke: { width: 2, position: 'inside' } }).above
    expect(px(inside, 10, 15)[3]).toBe(255)
    expect(px(inside, 15, 15)[3]).toBe(0)
    expect(px(inside, 9, 15)[3]).toBe(0)
    const centre = renderStyles(sq, { stroke: { width: 2, position: 'center' } }).above
    expect(px(centre, 9, 15)[3]).toBe(255)
    expect(px(centre, 10, 15)[3]).toBe(255)
    expect(px(centre, 15, 15)[3]).toBe(0)
  })

  it('overlays colour and gradient, glows and shades', () => {
    expect(px(renderStyles(sq, { colorOverlay: { color: '#00ff00' } }).above, 15, 15)).toEqual([0, 255, 0, 255])
    const grad = renderStyles(sq, { gradientOverlay: { angle: 90 } }).above
    expect(luma(px(grad, 15, 10))).toBeGreaterThan(luma(px(grad, 15, 19)) + 100)
    const glow = renderStyles(sq, { outerGlow: { size: 6 } }).below
    expect(px(glow, 8, 15)[3]).toBeGreaterThan(px(glow, 5, 15)[3])
    expect(px(glow, 5, 15)[3]).toBeGreaterThan(0)
    const ig = renderStyles(sq, { innerGlow: { size: 4 } }).above
    expect(px(ig, 10, 15)[3]).toBeGreaterThan(px(ig, 15, 15)[3])
    const is = renderStyles(sq, { innerShadow: { distance: 3, size: 0, opacity: 1 } }).above
    expect(px(is, 11, 11)[3]).toBe(255)
    expect(px(is, 18, 18)[3]).toBe(0)
  })

  it('bevels with the light from the upper left and skips styles that are off', () => {
    const { aboveList } = renderStyles(sq, { bevel: { size: 4 } })
    const hi = aboveList.find((e) => e.name === 'bevelHighlight').pixels
    const lo = aboveList.find((e) => e.name === 'bevelShadow').pixels
    expect(px(hi, 10, 12)[3]).toBeGreaterThan(px(hi, 19, 17)[3])
    expect(px(lo, 19, 17)[3]).toBeGreaterThan(px(lo, 10, 12)[3])
    expect(renderStyles(sq, { stroke: { on: false } }).aboveList).toHaveLength(0)
    expect(Object.keys(STYLE_DEFAULTS)).toHaveLength(8)
  })

  it('places a smaller layer at x, y in a bigger frame', () => {
    const small = square(4, 4, 0, 0, 4)
    const out = renderStyles(small, { colorOverlay: {} }, { width: 20, height: 20, x: 10, y: 5 }).above
    expect(out.width).toBe(20)
    expect(px(out, 11, 6)[3]).toBe(255)
    expect(px(out, 1, 1)[3]).toBe(0)
  })
})

describe('compose3', () => {
  const white = makePixels(8, 8, [255, 255, 255, 255])
  const red = makePixels(8, 8, [255, 0, 0, 255])

  it('matches compose2 for plain layers and adjustments', () => {
    const layers = [
      { id: 'a', kind: 'image', pixels: ramp(8, 8), opacity: 1, blend: 'normal', visible: true },
      { id: 'b', kind: 'image', pixels: square(8, 8, 2, 2, 3), opacity: 0.6, blend: 'multiply', visible: true, x: 1 },
      { id: 'c', kind: 'adjustment', adjustment: { type: 'levels', params: { gamma: 1.4 } }, opacity: 0.8, visible: true },
    ]
    const a = compose3(layers, 8, 8)
    const b = compose2(layers, 8, 8)
    for (let i = 0; i < a.data.length; i++) expect(Math.abs(a.data[i] - b.data[i])).toBeLessThanOrEqual(1)
  })

  it('composites an isolated group with its own opacity, and a pass-through group that reaches below', () => {
    const base = { id: 'bg', kind: 'image', pixels: white, opacity: 1, blend: 'normal', visible: true }
    const group = { id: 'g', kind: 'group', opacity: 0.5, blend: 'normal', visible: true }
    const child = { id: 'c', kind: 'image', parent: 'g', pixels: red, opacity: 1, blend: 'normal', visible: true }
    expect(near(px(compose3([base, group, child], 8, 8), 3, 3), [255, 128, 128, 255])).toBe(true)
    const inv = { id: 'i', kind: 'adjustment', parent: 'g2', adjustment: { type: 'invert' }, opacity: 1, visible: true }
    const iso = { id: 'g2', kind: 'group', opacity: 1, blend: 'normal', visible: true }
    expect(px(compose3([base, iso, inv], 8, 8), 0, 0)).toEqual([255, 255, 255, 255])
    const pass = { ...iso, blend: 'passThrough' }
    expect(px(compose3([base, pass, inv], 8, 8), 0, 0)).toEqual([0, 0, 0, 255])
    const hidden = { ...group, visible: false }
    expect(px(compose3([base, hidden, child], 8, 8), 3, 3)).toEqual([255, 255, 255, 255])
    const masked = { ...group, opacity: 1, mask: new Uint8Array(64) }
    expect(px(compose3([base, masked, child], 8, 8), 3, 3)).toEqual([255, 255, 255, 255])
  })

  it('clips a layer to the alpha of the layer below it', () => {
    const base = { id: 'b', kind: 'image', pixels: square(8, 8, 2, 2, 3, [0, 0, 255, 255]), opacity: 1, blend: 'normal', visible: true }
    const clipped = { id: 'c', kind: 'image', pixels: red, opacity: 1, blend: 'normal', visible: true, clipped: true }
    const out = compose3([base, clipped], 8, 8)
    expect(px(out, 3, 3)).toEqual([255, 0, 0, 255])
    expect(px(out, 0, 0)[3]).toBe(0)
    const hiddenBase = compose3([{ ...base, visible: false }, clipped], 8, 8)
    expect(px(hiddenBase, 3, 3)[3]).toBe(0)
    // a clipped adjustment only changes the base's pixels
    const bg = { id: 'w', kind: 'image', pixels: white, opacity: 1, blend: 'normal', visible: true }
    const adj = { id: 'a', kind: 'adjustment', adjustment: { type: 'invert' }, opacity: 1, visible: true, clipped: true }
    const both = compose3([bg, base, adj], 8, 8)
    expect(px(both, 3, 3)).toEqual([255, 255, 0, 255])
    expect(px(both, 0, 0)).toEqual([255, 255, 255, 255])
  })

  it('draws layer styles and every blend mode', () => {
    const layer = { id: 's', kind: 'image', pixels: square(16, 16, 5, 5, 6), opacity: 1, blend: 'normal', visible: true, styles: { stroke: { width: 1, color: '#00ff00' } } }
    const out = compose3([layer], 16, 16)
    expect(px(out, 4, 7)).toEqual([0, 255, 0, 255])
    expect(px(out, 7, 7)).toEqual([255, 0, 0, 255])
    for (const blend of BLEND_MODES) {
      const r = compose3([{ id: 'w', kind: 'image', pixels: ramp(8, 8), opacity: 1, visible: true }, { id: 'x', kind: 'image', pixels: square(8, 8, 1, 1, 5, [90, 200, 40, 200]), opacity: 0.8, blend, visible: true }], 8, 8)
      expect(r.width).toBe(8)
    }
    // a child whose parent is missing is drawn at the root
    expect(px(compose3([{ id: 'o', kind: 'image', parent: 'nope', pixels: red, opacity: 1, visible: true }], 8, 8), 0, 0)).toEqual([255, 0, 0, 255])
  })

  it('runs engine2 and engine3 adjustments by name', () => {
    const p = ramp(6, 6)
    expect(px(applyAdjustment3(p, { type: 'invert' }), 0, 0).slice(0, 3)).toEqual([255, 255, 127])
    expect(px(applyAdjustment3(p, { type: 'curves', params: { rgb: [[0, 255], [255, 0]] } }), 0, 0).slice(0, 3)).toEqual([255, 255, 127])
    expect(px(applyAdjustment3(p, { type: 'pixelate', params: { cell: 6 } }), 0, 0)).toEqual(px(applyAdjustment3(p, { type: 'pixelate', params: { cell: 6 } }), 5, 5))
  })
})

describe('adjustments', () => {
  const grey = makePixels(4, 4, [128, 128, 128, 255])

  it('lists every adjustment with defaults that run', () => {
    expect(Object.keys(ADJUSTMENTS)).toHaveLength(14)
    const p = colourful()
    for (const [type, { params }] of Object.entries(ADJUSTMENTS)) {
      const out = applyAdjustment3(p, { type, params })
      expect(out.width).toBe(p.width)
    }
    for (const type of ['vibrance', 'colorBalance', 'selectiveColor', 'channelMixer']) {
      const out = applyAdjustment3(p, { type, params: ADJUSTMENTS[type].params })
      for (let i = 0; i < out.data.length; i++) expect(Math.abs(out.data[i] - p.data[i])).toBeLessThanOrEqual(1)
    }
  })

  it('vibrance lifts dull colours more than strong ones', () => {
    const p = makePixels(2, 1)
    p.data.set([140, 120, 120, 255, 255, 0, 0, 255])
    const out = vibrance(p, { vibrance: 100 })
    const gainDull = (px(out, 0, 0)[0] - px(out, 0, 0)[1]) / 20
    const gainStrong = (px(out, 1, 0)[0] - px(out, 1, 0)[1]) / 255
    expect(gainDull).toBeGreaterThan(1.5)
    expect(gainStrong).toBeLessThanOrEqual(1.01)
  })

  it('colour balance, black & white, photo filter and gradient map', () => {
    const cb = px(colorBalance(grey, { midtones: [60, 0, 0] }), 0, 0)
    expect(cb[0]).toBeGreaterThan(cb[2])
    expect(Math.abs(luma(cb) - 128)).toBeLessThan(2)
    const bw = makePixels(3, 1)
    bw.data.set([255, 0, 0, 255, 255, 255, 0, 255, 255, 255, 255, 255])
    const g = blackWhite(bw)
    expect(px(g, 0, 0).slice(0, 3)).toEqual([102, 102, 102])
    expect(px(g, 1, 0)[0]).toBe(153)
    expect(px(g, 2, 0)[0]).toBe(255)
    const tinted = px(blackWhite(bw, { tint: '#c08040' }), 2, 0)
    expect(tinted[0]).toBeGreaterThanOrEqual(tinted[2])
    const pf = px(photoFilter(grey, { density: 60 }), 0, 0)
    expect(pf[0]).toBeGreaterThan(pf[2])
    const gm = gradientMap(makePixels(1, 1, [255, 255, 255, 255]), { stops: [{ at: 0, color: '#000000' }, { at: 1, color: '#ff0000' }] })
    expect(near(px(gm, 0, 0), [255, 0, 0, 255])).toBe(true)
  })

  it('selective colour, channel mixer, posterize, threshold, invert', () => {
    const p = makePixels(2, 1)
    p.data.set([200, 50, 50, 255, 50, 50, 200, 255])
    const sc = selectiveColor(p, { reds: { c: -100 } })
    expect(px(sc, 0, 0)[0]).toBeGreaterThan(200)
    expect(px(sc, 1, 0)).toEqual([50, 50, 200, 255])
    expect(px(channelMixer(p, { red: [0, 0, 100, 0], blue: [100, 0, 0, 0] }), 0, 0).slice(0, 3)).toEqual([50, 50, 200])
    const ps = posterize(ramp(8, 8), { levels: 2 })
    for (let i = 0; i < ps.data.length; i += 4) expect([0, 255]).toContain(ps.data[i])
    expect(px(threshold(grey, { level: 129 }), 0, 0)[0]).toBe(0)
    expect(px(threshold(grey, { level: 128 }), 0, 0)[0]).toBe(255)
    expect(px(invert(p), 0, 0).slice(0, 3)).toEqual([55, 205, 205])
    const mask = Uint8Array.from([0, 255])
    expect(px(invert(p, {}, mask), 0, 0)).toEqual([200, 50, 50, 255])
  })

  it('exposure, shadows/highlights and brightness/contrast', () => {
    expect(px(exposure(grey, {}), 0, 0)).toEqual([128, 128, 128, 255])
    expect(px(exposure(grey, { exposure: 1 }), 0, 0)[0]).toBeGreaterThan(160)
    expect(px(exposure(grey, { gamma: 2 }), 0, 0)[0]).toBeGreaterThan(128)
    const dark = makePixels(6, 6, [30, 30, 30, 255])
    expect(px(shadowsHighlights(dark, { shadows: 80, radius: 4 }), 3, 3)[0]).toBeGreaterThan(50)
    const bright = makePixels(6, 6, [240, 240, 240, 255])
    expect(px(shadowsHighlights(bright, { shadows: 0, highlights: 80, radius: 4 }), 3, 3)[0]).toBeLessThan(220)
    expect(px(brightnessContrast(grey, { brightness: 100 }), 0, 0)[0]).toBeGreaterThan(140)
    const two = makePixels(2, 1)
    two.data.set([100, 100, 100, 255, 160, 160, 160, 255])
    const c = brightnessContrast(two, { contrast: 50 })
    expect(px(c, 1, 0)[0] - px(c, 0, 0)[0]).toBeGreaterThan(60)
  })

  it('applies six film looks', () => {
    expect(Object.keys(LOOKS)).toHaveLength(6)
    const p = colourful()
    for (const look of Object.keys(LOOKS)) {
      const out = colorLookup(p, { look })
      let diff = 0
      for (let i = 0; i < out.data.length; i++) diff += Math.abs(out.data[i] - p.data[i])
      expect(diff).toBeGreaterThan(0)
    }
    const noir = px(colorLookup(p, { look: 'noir' }), 5, 5)
    expect(noir[0]).toBe(noir[1])
    expect(colorLookup(p, { look: 'portra', amount: 0 }).data).toEqual(p.data)
  })
})

describe('filters', () => {
  it('lists every filter with defaults that run', () => {
    expect(Object.keys(FILTERS)).toHaveLength(11)
    const p = colourful(10, 10)
    for (const [type, { params }] of Object.entries(FILTERS)) expect(applyFilter(p, { type, params }).width).toBe(10)
  })

  it('motion blur smears along its angle only', () => {
    const p = makePixels(20, 20, [0, 0, 0, 255])
    for (let y = 0; y < 20; y++) for (let x = 10; x < 20; x++) p.data.set([255, 255, 255, 255], (y * 20 + x) * 4)
    const h = motionBlur(p, { angle: 0, distance: 6 })
    expect(px(h, 9, 10)[0]).toBeGreaterThan(50)
    const v = motionBlur(p, { angle: 90, distance: 6 })
    expect(px(v, 9, 10)[0]).toBe(0)
  })

  it('radial blur spins the corners and leaves the middle', () => {
    const p = colourful(16, 16)
    const out = radialBlur(p, { mode: 'spin', amount: 40 })
    const zoom = radialBlur(p, { mode: 'zoom', amount: 40 })
    expect(px(out, 0, 0)).not.toEqual(px(p, 0, 0))
    expect(px(zoom, 15, 15)).not.toEqual(px(p, 15, 15))
  })

  it('surface blur smooths noise but keeps the edge; lens blur follows depth', () => {
    const p = makePixels(16, 8)
    for (let y = 0; y < 8; y++) for (let x = 0; x < 16; x++) {
      const v = (x < 8 ? 60 : 200) + ((x * 7 + y * 3) % 5) * 3
      p.data.set([v, v, v, 255], (y * 16 + x) * 4)
    }
    const s = surfaceBlur(p, { radius: 2, threshold: 20 })
    expect(variance(s, 1, 1, 6, 7)).toBeLessThan(variance(p, 1, 1, 6, 7))
    expect(px(s, 7, 4)[0]).toBeLessThan(90)
    expect(px(s, 8, 4)[0]).toBeGreaterThan(180)
    const depth = new Uint8Array(16 * 8)
    for (let j = 0; j < depth.length; j++) depth[j] = j % 16 < 8 ? 0 : 255
    const c = makePixels(16, 8)
    for (let j = 0; j < 128; j++) c.data.set((j + Math.floor(j / 16)) % 2 ? [255, 255, 255, 255] : [0, 0, 0, 255], j * 4)
    const lb = lensBlur(c, { depthMask: depth, radius: 6 })
    expect(px(lb, 2, 2)).toEqual(px(c, 2, 2))
    expect(variance(lb, 10, 1, 15, 7)).toBeLessThan(variance(c, 10, 1, 15, 7) / 4)
  })

  it('high pass, pixelate, emboss, find edges, oil paint', () => {
    const flat = makePixels(8, 8, [90, 90, 90, 255])
    expect(px(highPass(flat), 4, 4).slice(0, 3)).toEqual([128, 128, 128])
    const pix = pixelate(ramp(8, 8), { cell: 4 })
    expect(px(pix, 0, 0)).toEqual(px(pix, 3, 3))
    expect(px(pix, 0, 0)).not.toEqual(px(pix, 4, 0))
    expect(px(emboss(flat), 4, 4)[0]).toBe(128)
    expect(px(findEdges(flat), 4, 4)[0]).toBe(255)
    const edge = makePixels(8, 8, [0, 0, 0, 255])
    for (let y = 0; y < 8; y++) for (let x = 4; x < 8; x++) edge.data.set([255, 255, 255, 255], (y * 8 + x) * 4)
    expect(px(findEdges(edge), 4, 4)[0]).toBeLessThan(100)
    expect(px(oilPaint(flat), 3, 3)).toEqual([90, 90, 90, 255])
  })

  it('grain is repeatable; develop warms, dehazes and adds local contrast', () => {
    const grey = makePixels(12, 12, [128, 128, 128, 255])
    const a = addGrain(grey, { amount: 30, seed: 3 })
    expect(a.data).toEqual(addGrain(grey, { amount: 30, seed: 3 }).data)
    expect(a.data).not.toEqual(grey.data)
    expect(addGrain(grey, { amount: 30, size: 3 }).width).toBe(12)
    expect(addGrain(grey, { amount: 0 }).data).toEqual(grey.data)
    const warm = px(develop(grey, { temp: 50 }), 2, 2)
    expect(warm[0]).toBeGreaterThan(warm[2])
    const hazy = makePixels(20, 20)
    for (let y = 0; y < 20; y++) for (let x = 0; x < 20; x++) {
      const v = x < 10 ? 150 : 200
      hazy.data.set([v, v, v + 10, 255], (y * 20 + x) * 4)
    }
    const dh = develop(hazy, { dehaze: 80 })
    expect(px(dh, 15, 10)[0] - px(dh, 3, 10)[0]).toBeGreaterThan(50)
    const cl = develop(hazy, { clarity: 80, texture: 50 })
    expect(cl.width).toBe(20)
  })
})

describe('warps', () => {
  it('an untouched mesh renders the picture unchanged', () => {
    const p = colourful(16, 16)
    const mesh = liquifyMesh(16, 16, 4)
    expect(mesh.cols).toBe(5)
    expect(applyMesh(p, mesh).data).toEqual(p.data)
  })

  it('push drags content along the stroke; bloat and pucker swell and pinch; reconstruct relaxes', () => {
    const p = makePixels(32, 32, [255, 255, 255, 255])
    for (let y = 0; y < 32; y++) p.data.set([255, 0, 0, 255], (y * 32 + 10) * 4)
    const mesh = liquifyStroke(liquifyMesh(32, 32, 4), { mode: 'push', points: [[10, 16], [16, 16]], size: 16, pressure: 1 })
    const out = applyMesh(p, mesh)
    let at = -1
    for (let x = 0; x < 32; x++) if (px(out, x, 16)[1] < 128) at = x
    expect(at).toBeGreaterThan(11)
    const dot = makePixels(32, 32, [255, 255, 255, 255])
    for (let y = 13; y < 19; y++) for (let x = 13; x < 19; x++) dot.data.set([0, 0, 0, 255], (y * 32 + x) * 4)
    const count = (q) => {
      let n = 0
      for (let i = 0; i < q.data.length; i += 4) if (q.data[i] < 128) n++
      return n
    }
    const big = applyMesh(dot, liquifyStroke(liquifyMesh(32, 32, 2), { mode: 'bloat', points: [[16, 16], [16, 16], [16, 16]], size: 24, pressure: 1 }))
    const small = applyMesh(dot, liquifyStroke(liquifyMesh(32, 32, 2), { mode: 'pucker', points: [[16, 16], [16, 16], [16, 16]], size: 24, pressure: 1 }))
    expect(count(big)).toBeGreaterThan(count(dot))
    expect(count(small)).toBeLessThan(count(dot))
    const back = liquifyStroke(mesh, { mode: 'reconstruct', points: [[13, 16]], size: 30, pressure: 1 })
    const sum = (m) => m.dx.reduce((a, b) => a + Math.abs(b), 0)
    expect(sum(back)).toBeLessThan(sum(mesh))
  })

  it('warps by a Bezier grid and pins corners by homography', () => {
    const p = colourful(16, 16)
    const same = meshWarp(p, defaultWarpGrid(16, 16))
    for (let i = 0; i < p.data.length; i++) expect(Math.abs(same.data[i] - p.data[i])).toBeLessThanOrEqual(2)
    const moved = meshWarp(p, defaultWarpGrid(16, 16).map(([x, y]) => [x + 4, y]))
    expect(px(moved, 1, 5)[3]).toBe(0)
    expect(near(px(moved, 6, 5), px(p, 2, 5))).toBe(true)
    expect(() => meshWarp(p, [[0, 0]])).toThrow()
    const Hm = homography([[0, 0], [1, 0], [1, 1], [0, 1]], [[0, 0], [2, 0], [2, 2], [0, 2]])
    expect(applyHomography(Hm, 0.5, 0.5).map((v) => Math.round(v * 1000) / 1000)).toEqual([1, 1])
    const ident = perspective(p, [[0, 0], [16, 0], [16, 16], [0, 16]])
    for (let i = 0; i < p.data.length; i++) expect(Math.abs(ident.data[i] - p.data[i])).toBeLessThanOrEqual(1)
    const half = perspective(p, [[0, 0], [8, 0], [8, 8], [0, 8]])
    expect(px(half, 12, 12)[3]).toBe(0)
    expect(px(half, 4, 4)[3]).toBe(255)
  })
})

describe('retouch brushes', () => {
  const grey = makePixels(20, 20, [128, 128, 128, 255])
  const stroke = { points: [[10, 10]], size: 8, hardness: 1, strength: 0.5 }

  it('dodge, burn and sponge', () => {
    expect(px(dodge(grey, stroke), 10, 10)[0]).toBeGreaterThan(150)
    expect(px(dodge(grey, stroke), 1, 1)[0]).toBe(128)
    expect(px(burn(grey, stroke), 10, 10)[0]).toBeLessThan(100)
    const dark = makePixels(20, 20, [20, 20, 20, 255])
    expect(px(dodge(dark, { ...stroke, range: 'highlights' }), 10, 10)[0]).toBeLessThan(25)
    expect(px(dodge(dark, { ...stroke, range: 'shadows' }), 10, 10)[0]).toBeGreaterThan(60)
    const red = makePixels(20, 20, [200, 50, 50, 255])
    const d = px(sponge(red, { ...stroke, strength: 1, mode: 'desaturate' }), 10, 10)
    expect(d[0]).toBe(d[1])
    const s = px(sponge(red, { ...stroke, mode: 'saturate' }), 10, 10)
    expect(s[0] - s[1]).toBeGreaterThan(150)
  })

  it('smudge drags colour along the stroke', () => {
    const p = makePixels(30, 10, [255, 255, 255, 255])
    for (let y = 0; y < 10; y++) for (let x = 0; x < 8; x++) p.data.set([255, 0, 0, 255], (y * 30 + x) * 4)
    const out = smudge(p, { points: [[5, 5], [16, 5]], size: 6, strength: 0.8 })
    expect(px(out, 12, 5)[1]).toBeLessThan(200)
    expect(px(out, 12, 0)[1]).toBe(255)
  })

  it('blur and sharpen brushes work inside the stroke only', () => {
    const c = makePixels(20, 20)
    for (let j = 0; j < 400; j++) c.data.set((j + Math.floor(j / 20)) % 2 ? [255, 255, 255, 255] : [0, 0, 0, 255], j * 4)
    const b = blurBrush(c, { points: [[10, 10]], size: 10, hardness: 1, strength: 1 })
    expect(variance(b, 8, 8, 12, 12)).toBeLessThan(variance(c, 8, 8, 12, 12) / 4)
    expect(px(b, 1, 1)).toEqual(px(c, 1, 1))
    const e = makePixels(20, 20, [100, 100, 100, 255])
    for (let y = 0; y < 20; y++) for (let x = 10; x < 20; x++) e.data.set([150, 150, 150, 255], (y * 20 + x) * 4)
    const sh = sharpenBrush(e, { points: [[10, 10]], size: 8, hardness: 1, strength: 1 })
    expect(px(sh, 10, 10)[0]).toBeGreaterThan(150)
    expect(px(sh, 9, 10)[0]).toBeLessThan(100)
  })

  it('spot heal removes a blemish, patch rebuilds a selection seamlessly', () => {
    const p = ramp(48, 48)
    for (let y = 22; y < 26; y++) for (let x = 22; x < 26; x++) p.data.set([0, 0, 0, 255], (y * 48 + x) * 4)
    const clean = ramp(48, 48)
    const healed = spotHeal(p, { points: [[24, 24]], size: 10 })
    expect(Math.abs(luma(px(healed, 24, 24)) - luma(px(clean, 24, 24)))).toBeLessThan(25)
    const mask = new Uint8Array(48 * 48)
    for (let y = 20; y < 28; y++) for (let x = 20; x < 28; x++) mask[y * 48 + x] = 255
    const pt = patch(p, mask, { dx: 12, dy: 0 })
    expect(Math.abs(luma(px(pt, 24, 24)) - luma(px(clean, 24, 24)))).toBeLessThan(12)
    expect(px(pt, 5, 5)).toEqual(px(p, 5, 5))
  })
})

describe('selections', () => {
  const scene = () => {
    const p = makePixels(40, 40, [30, 60, 200, 255])
    for (let y = 12; y < 28; y++) for (let x = 12; x < 28; x++) p.data.set([220, 40, 30, 255], (y * 40 + x) * 4)
    return p
  }

  it('quick select grows from a stroke, adds and subtracts', () => {
    const p = scene()
    const m = quickSelect(p, [[[18, 20], [22, 20]]], { size: 4 })
    expect(m[20 * 40 + 13]).toBeGreaterThan(200)
    expect(m[20 * 40 + 26]).toBeGreaterThan(200)
    expect(m[5 * 40 + 5]).toBeLessThan(50)
    const plusBg = quickSelect(p, [[3, 3]], { size: 4, prevMask: m })
    expect(plusBg[5 * 40 + 5]).toBeGreaterThan(200)
    expect(plusBg[20 * 40 + 20]).toBeGreaterThan(200)
    const minus = quickSelect(p, [{ points: [[20, 20]], size: 4 }], { add: false, prevMask: plusBg })
    expect(minus[20 * 40 + 20]).toBeLessThan(50)
    expect(minus[5 * 40 + 5]).toBeGreaterThan(200)
    expect(quickSelect(p, [], {}).every((v) => v === 0)).toBe(true)
  })

  it('object select finds the subject inside the rectangle', () => {
    const m = objectSelect(scene(), { x: 6, y: 6, w: 28, h: 28 })
    expect(m[20 * 40 + 20]).toBeGreaterThan(200)
    expect(m[8 * 40 + 8]).toBeLessThan(50)
  })
})

describe('guides and artboards', () => {
  it('snaps a value and a box', () => {
    expect(snapTo(103, [0, 100, 200], 5)).toEqual({ value: 100, snapped: true, target: 100 })
    expect(snapTo(110, [0, 100], 5)).toEqual({ value: 110, snapped: false, target: null })
    const guide = { value: 50, kind: 'guide' }
    expect(snapTo(48, [guide], 4).target).toBe(guide)
    const b = snapBox({ x: 47, y: 10, w: 20, h: 20 }, { xs: [70], ys: [21] }, 4)
    expect(b.x).toBe(50)
    expect(b.y).toBe(11)
    expect(b.snapX).toBe(70)
  })

  it('has artboard presets and fits pictures', () => {
    expect(artboardPresets.map((p) => p.width)).toEqual([1024, 512, 192, 180, 48, 1200, 1080, 1920])
    expect(fitInto(200, 100, 100, 100, 'contain')).toEqual({ x: 0, y: 25, width: 100, height: 50, scale: 0.5 })
    expect(fitInto(200, 100, 100, 100, 'cover')).toEqual({ x: -50, y: 0, width: 200, height: 100, scale: 1 })
  })
})
