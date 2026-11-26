import { describe, expect, it } from 'vitest'
import {
  TileCache,
  ZOOM_LEVELS,
  applyRect,
  clampZoom,
  fitPage,
  fitWidth,
  invert,
  itemBounds,
  multiply,
  pageLayout,
  pageToScreen,
  pathBounds,
  screenToPage,
  snapZoom,
  strokeReach,
  tilesFor,
  visibleItems,
  visiblePageRect,
  worldBounds,
  zoomIn,
  zoomOut,
} from './tiles.js'

// Every page point of `v` (sampled) lies in some tile.
function covers(tiles, v) {
  for (let i = 0; i <= 10; i++) {
    for (let j = 0; j <= 10; j++) {
      const x = v.x + (v.w * i) / 10, y = v.y + (v.h * j) / 10
      if (!tiles.some((t) => x >= t.rect.x && x <= t.rect.x + t.rect.w && y >= t.rect.y && y <= t.rect.y + t.rect.h)) return false
    }
  }
  return true
}

describe('tilesFor', () => {
  it('one tile covers a small view at 100%', () => {
    const t = tilesFor({ x: 0, y: 0, w: 100, h: 100 }, 1, 512, 1)
    expect(t.length).toBe(1)
    expect(t[0]).toMatchObject({ key: '1/0/0', col: 0, row: 0, rect: { x: 0, y: 0, w: 512, h: 512 }, px: { x: 0, y: 0, w: 512, h: 512 } })
  })

  it('covers the view at several zooms and dprs, with tiles shrinking in page space as zoom grows', () => {
    const page = { w: 612, h: 792 }
    const v = { x: 30, y: 40, w: 400, h: 300 }
    for (const zoom of [0.25, 0.5, 1, 1.5, 2, 4, 8]) {
      for (const dpr of [1, 2]) {
        const tiles = tilesFor(v, zoom, 512, dpr, page)
        expect(covers(tiles, v)).toBe(true)
        const side = 512 / (zoom * dpr)
        for (const t of tiles) {
          expect(t.rect.x).toBeCloseTo(t.col * side)
          expect(t.px.x).toBe(t.col * 512)
          expect(t.rect.w).toBeLessThanOrEqual(side + 1e-9)
          expect(t.rect.x + t.rect.w).toBeLessThanOrEqual(page.w + 1e-9)
          expect(t.key.startsWith(`${zoom * dpr}/`)).toBe(true)
        }
      }
    }
    // 8x at dpr 2: 16 device px per point -> 32 points per tile; x 30..430 -> cols 0..13, y 40..340 -> rows 1..10
    expect(tilesFor(v, 8, 512, 2, page).length).toBe(14 * 10)
    // 1x at dpr 1: one 512-point tile covers it all
    expect(tilesFor(v, 1, 512, 1, page).length).toBe(1)
  })

  it('cuts the last row and column to the page edge', () => {
    const tiles = tilesFor({ x: 0, y: 0, w: 612, h: 792 }, 1, 512, 1, { w: 612, h: 792 })
    expect(tiles.map((t) => t.key)).toEqual(['1/0/0', '1/1/0', '1/0/1', '1/1/1'])
    const last = tiles[3]
    expect(last.rect).toEqual({ x: 512, y: 512, w: 100, h: 280 })
    expect(last.px).toEqual({ x: 512, y: 512, w: 100, h: 280 })
  })

  it('a view edge exactly on a tile boundary does not add a tile', () => {
    expect(tilesFor({ x: 0, y: 0, w: 512, h: 512 }, 1, 512, 1).length).toBe(1)
    expect(tilesFor({ x: 0, y: 0, w: 256, h: 256 }, 2, 512, 1).length).toBe(1)
  })

  it('is empty for an empty view or one off the page', () => {
    expect(tilesFor({ x: 0, y: 0, w: 0, h: 10 }, 1)).toEqual([])
    expect(tilesFor({ x: 700, y: 0, w: 100, h: 10 }, 1, 512, 1, { w: 612, h: 792 })).toEqual([])
    expect(tilesFor({ x: 0, y: 0, w: 10, h: 10 }, 0)).toEqual([])
  })
})

describe('bounds', () => {
  it('scans path data for rough bounds, control points included', () => {
    expect(pathBounds('M10 20L30 40Z')).toEqual({ x: 10, y: 20, w: 20, h: 20 })
    expect(pathBounds('M0 0C-5 10 15 -2.5 10 0Z')).toEqual({ x: -5, y: -2.5, w: 20, h: 12.5 })
    expect(pathBounds('M1.5e2 -3L.5 4Q7 8 9 1')).toEqual({ x: 0.5, y: -3, w: 149.5, h: 11 })
    expect(pathBounds('')).toBe(null)
    expect(pathBounds('Z')).toBe(null)
  })

  it('grows a stroked path by the stroke reach', () => {
    expect(strokeReach({ w: 2, cap: 'butt', join: 'round' })).toBe(1)
    expect(strokeReach({ w: 2, cap: 'square', join: 'bevel' })).toBeCloseTo(Math.SQRT2)
    expect(strokeReach({ w: 2, cap: 'butt', join: 'miter', ml: 4 })).toBe(4)
    expect(itemBounds({ t: 'path', d: 'M0 0L10 0', stroke: { w: 2, cap: 'round', join: 'round' } })).toEqual({ x: -1, y: -1, w: 12, h: 2 })
  })

  it('gives glyph runs a generous em box and state items none', () => {
    expect(itemBounds({ t: 'glyphs', size: 10, g: [1, 0, 100, 2, 20, 100] })).toEqual({ x: -5, y: 88, w: 40, h: 17 })
    expect(itemBounds({ t: 'image', r: [1, 2, 3, 4] })).toEqual({ x: 1, y: 2, w: 3, h: 4 })
    expect(itemBounds({ t: 'clip', d: 'M0 0L1 1' })).toBe(null)
    expect(itemBounds({ t: 'glyphs', size: 10, g: [] })).toBe(null)
  })

  it('world bounds apply transforms and clips like display.rs', () => {
    const list = {
      w: 100,
      h: 100,
      items: [
        { t: 'transform', m: [2, 0, 0, 2, 10, 0] },
        { t: 'path', d: 'M0 0L10 10', fill: { k: 'solid', c: '#000' } },
        { t: 'untransform' },
        { t: 'clip', d: 'M0 0L50 0L50 50L0 50Z' },
        { t: 'image', image: 1, r: [40, 40, 20, 20] },
        { t: 'image', image: 2, r: [60, 60, 5, 5] },
        { t: 'unclip' },
        { t: 'image', image: 3, r: [60, 60, 5, 5] },
      ],
    }
    const b = worldBounds(list)
    expect(b[0]).toBe(null)
    expect(b[1]).toEqual({ x: 10, y: 0, w: 20, h: 20 })
    expect(b[4]).toEqual({ x: 40, y: 40, w: 10, h: 10 })
    expect(b[5]).toBe(null) // clipped away
    expect(b[7]).toEqual({ x: 60, y: 60, w: 5, h: 5 })

    const vis = visibleItems(b, { x: 0, y: 0, w: 45, h: 45 }, list.items)
    expect(vis).toEqual([0, 1, 2, 3, 4, 6])
    // without the items every null box is kept (it may be a state item)
    expect(visibleItems(b, [0, 0, 45, 45])).toEqual([0, 1, 2, 3, 4, 5, 6])
    // a box touching the tile edge counts
    expect(visibleItems([[45, 0, 5, 5]], { x: 0, y: 0, w: 45, h: 45 })).toEqual([0])
  })

  it('matrix helpers invert and map rects', () => {
    const m = [2, 1, -1, 3, 5, -7]
    const inv = invert(m)
    const id = multiply(m, inv)
    ;[1, 0, 0, 1, 0, 0].forEach((v, i) => expect(id[i]).toBeCloseTo(v))
    expect(invert([0, 0, 0, 1, 0, 0])).toBe(null)
    expect(applyRect([1, 0, 0, 1, 3, 4], { x: 1, y: 1, w: 2, h: 2 })).toEqual({ x: 4, y: 5, w: 2, h: 2 })
  })
})

describe('TileCache', () => {
  const bmp = (w, h, log) => ({ width: w, height: h, close: () => log.push(`${w}x${h}`) })

  it('evicts the least recently used under its byte budget and closes what it drops', () => {
    const log = []
    const c = new TileCache({ budget: 3 * 100 * 100 * 4 })
    c.set('a', bmp(100, 100, log))
    c.set('b', bmp(100, 100, log))
    c.set('c', bmp(100, 100, log))
    expect(c.bytes).toBe(120000)
    c.get('a') // a is now the newest
    c.set('d', bmp(100, 100, log))
    expect(c.keys()).toEqual(['c', 'a', 'd'])
    expect(c.has('b')).toBe(false)
    expect(log.length).toBe(1)
    expect(c.bytes).toBeLessThanOrEqual(c.budget)
  })

  it('never evicts the entry just put in, even over budget', () => {
    const c = new TileCache({ budget: 10 })
    c.set('a', { width: 1, height: 1 })
    c.set('big', { width: 100, height: 100 })
    expect(c.keys()).toEqual(['big'])
    expect(c.size).toBe(1)
  })

  it('replaces, deletes, prunes and clears with the byte count kept true', () => {
    const log = []
    const c = new TileCache({ budget: 1e9 })
    c.set('1/0/0', bmp(10, 10, log))
    c.set('1/0/0', bmp(20, 10, log))
    expect(c.bytes).toBe(800)
    expect(log).toEqual(['10x10'])
    c.set('2/0/0', bmp(10, 10, log), 7)
    expect(c.bytes).toBe(807)
    expect(c.peek('1/0/0').width).toBe(20)
    c.prune((k) => !k.startsWith('2/'))
    expect(c.keys()).toEqual(['2/0/0'])
    expect(c.delete('2/0/0')).toBe(true)
    expect(c.delete('2/0/0')).toBe(false)
    expect(c.bytes).toBe(0)
    c.set('x', bmp(1, 1, log))
    c.clear()
    expect(c.size).toBe(0)
    expect(c.bytes).toBe(0)
  })
})

describe('zoom', () => {
  it('clamps to 25%..800% and snaps to the nearest level by ratio', () => {
    expect(clampZoom(0.1)).toBe(0.25)
    expect(clampZoom(20)).toBe(8)
    expect(clampZoom(NaN)).toBe(1)
    expect(snapZoom(1.04)).toBe(1)
    expect(snapZoom(0.3)).toBe(0.33)
    expect(snapZoom(7.2)).toBe(8)
    expect(snapZoom(100)).toBe(8)
    for (const l of ZOOM_LEVELS) expect(snapZoom(l)).toBe(l)
  })

  it('steps through the levels', () => {
    expect(zoomIn(1)).toBe(1.1)
    expect(zoomIn(1.05)).toBe(1.1)
    expect(zoomIn(8)).toBe(8)
    expect(zoomOut(1)).toBe(0.9)
    expect(zoomOut(0.25)).toBe(0.25)
    expect(zoomOut(0.3)).toBe(0.25)
  })

  it('fits the width and the whole page', () => {
    expect(fitWidth(612, 660, 24)).toBe(1)
    expect(fitWidth(612, 1272, 24)).toBe(2)
    expect(fitWidth(612, 10, 24)).toBe(0.25)
    expect(fitPage(612, 792, 1272, 840, 24)).toBe(1) // height bound
    expect(fitPage(612, 792, 660, 5000, 24)).toBe(1) // width bound
    expect(fitPage(0, 792, 660, 840)).toBe(1)
  })
})

describe('page layout', () => {
  it('centres a small page and pads a large one, and maps points both ways', () => {
    const small = pageLayout({ pageW: 100, pageH: 100, zoom: 1, viewW: 400, viewH: 300, pad: 20 })
    expect(small).toMatchObject({ left: 150, top: 100, contentW: 400, contentH: 300 })
    const big = pageLayout({ pageW: 612, pageH: 792, zoom: 2, viewW: 800, viewH: 600, scrollX: 100, scrollY: 50, pad: 20 })
    expect(big).toMatchObject({ left: -80, top: -30, contentW: 1264, contentH: 1624 })
    const p = screenToPage(120, 170, big)
    expect(p).toEqual({ x: 100, y: 100 })
    expect(pageToScreen(p.x, p.y, big)).toEqual({ x: 120, y: 170 })
    expect(visiblePageRect(big, 800, 600, 612, 792)).toEqual({ x: 40, y: 15, w: 400, h: 300 })
    expect(visiblePageRect({ zoom: 1, left: 1000, top: 0 }, 800, 600, 100, 100)).toBe(null)
  })
})
