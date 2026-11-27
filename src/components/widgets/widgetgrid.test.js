import { describe, expect, it } from 'vitest'
import seed from '../../themes/seed/theme.json'
import {
  COLS, MAX_H, boxOf, cellFromOldPlace, cellFromTheme, cellsOf, chosenSize, clampCell, columnsFor, completeGrid, cssPx, firstFit, geometry, gridOf,
  layoutFor, moveTo, overlapping, pack, reflow, resizeTo, settle, sizeChoices, snapOf, snapPatch, spanOf, tidy,
} from './widgetgrid.js'

// HOME at three Castle window widths: full screen, half screen, and a narrow window. The grid's room
// is HOME beside the chat panel (31% wide at 1.6% from the left, then 16px).
function ctxFor(width, height = 852) {
  const left = Math.round(width * 0.326 + 16)
  return { home: { width, height }, area: { left, top: Math.round(height * 0.03), width: Math.round(width * 0.98) - left, height: Math.round(height * 0.94) - 56 } }
}
const FULL = ctxFor(1440)
const HALF = ctxFor(720)
const NARROW = ctxFor(520)
const WIDTHS = [['full screen', FULL], ['half screen', HALF], ['narrow', NARROW]]

// The widgets HOME shows by default with the Seed theme (the ones in W4's screenshot and more).
const SHOWN = ['clock', 'gauges', 'disks', 'sentinel', 'automations', 'calendar', 'sweep', 'usage', 'today', 'projects', 'diary', 'growth', 'backup', 'database']
const FALLBACK = { sweep: { right: '31%', top: '22%' }, disks: { right: '2%', bottom: '4%' }, sentinel: { right: '43%', top: '22%' }, automations: { right: '2%', top: '38%' }, calendar: { right: '2%', top: '62%' } }
const themeFrom = (id) => ({ theme: seed.widgets[id] ?? FALLBACK[id] })

// W4 as Akasha saw it: the corners an older Castle kept, in percent of HOME (overlapping).
const OLD = {
  clock: { left: 33, top: 1 }, gauges: { left: 79, top: 1 }, today: { left: 51, top: 15 }, automations: { left: 72, top: 37 }, projects: { left: 34, top: 42 },
  calendar: { left: 50, top: 62 }, usage: { left: 0, top: 73 }, diary: { left: 0, top: 89 }, growth: { left: 47, top: 89 },
}
const oldFrom = (id) => ({ old: OLD[id], size: id === 'today' ? 'l' : 'm', theme: seed.widgets[id] ?? FALLBACK[id] })

const noOverlap = (list) => expect(overlapping(list)).toEqual([])
const inside = (list, cols) => list.forEach((c) => {
  expect(c.x).toBeGreaterThanOrEqual(0)
  expect(c.x + c.w).toBeLessThanOrEqual(cols)
  expect(Number.isInteger(c.x) && Number.isInteger(c.y) && Number.isInteger(c.w) && Number.isInteger(c.h)).toBe(true)
})

describe('the grid HOME draws', () => {
  it('has six columns at full screen, four at half screen and two in a narrow window', () => {
    expect(columnsFor(FULL.area.width)).toBe(6)
    expect(columnsFor(HALF.area.width)).toBe(4)
    expect(columnsFor(NARROW.area.width)).toBe(2)
    expect(columnsFor(0)).toBe(2)
  })

  it('fits the columns inside the room beside the chat panel and stops cells growing past a size', () => {
    for (const [, ctx] of WIDTHS) {
      const geo = geometry(ctx.area.width)
      expect(geo.width).toBeLessThanOrEqual(ctx.area.width + 0.01)
      expect(boxOf({ x: geo.cols - 1, y: 0, w: 1, h: 1 }, geo).left + geo.cell).toBeLessThanOrEqual(ctx.area.width + 0.01)
    }
    const wide = geometry(3000)
    expect(wide.cell).toBe(220)
    expect(wide.offset).toBeGreaterThan(0)
  })

  it('gives a box the gaps between its cells', () => {
    const geo = { cell: 100, gap: 10, row: 80, colPitch: 110, rowPitch: 90 }
    expect(boxOf({ x: 1, y: 2, w: 2, h: 3 }, geo)).toEqual({ left: 110, top: 180, width: 210, height: 260 })
  })
})

describe('a first start: every widget from its theme place, packed', () => {
  for (const [name, ctx] of WIDTHS) {
    it(`places every widget with no overlap (${name})`, () => {
      const grid = completeGrid({}, SHOWN, themeFrom, ctx)
      const list = cellsOf(grid)
      expect(list.map((c) => c.id).sort()).toEqual([...SHOWN].sort())
      noOverlap(list)
      inside(list, COLS)
      // ... and HOME draws it with no overlap at all three widths
      for (const [, at] of WIDTHS) {
        const cols = columnsFor(at.area.width)
        const drawn = layoutFor(list, { snap: true, cols })
        noOverlap(drawn)
        inside(drawn, cols)
        expect(drawn).toHaveLength(SHOWN.length)
      }
    })
  }

  it("keeps the theme's sense of place: the clock at the top, TODAY on the right", () => {
    const grid = completeGrid({}, SHOWN, themeFrom, FULL)
    expect(grid.clock.y).toBe(0)
    expect(grid.gauges.y).toBe(0)
    expect(grid.gauges.x).toBeGreaterThan(grid.clock.x)
  })

  it('lays the cards densely when the theme stacks them deeper than HOME is tall', () => {
    const grid = completeGrid({}, SHOWN, themeFrom, FULL)
    const rows = Math.max(...cellsOf(grid).map((c) => c.y + c.h))
    expect(rows).toBeLessThanOrEqual(Math.floor((FULL.area.height + 12) / 124) + 1)
    const tall = completeGrid({}, SHOWN, themeFrom, { ...FULL, area: { ...FULL.area, height: 5000 } })
    expect(tall.today.x + tall.today.w).toBe(6)
  })

  it('places a widget the theme has no place for at the first free spot', () => {
    const grid = completeGrid({}, ['clock', 'credit-suno'], themeFrom, FULL)
    noOverlap(cellsOf(grid))
    expect(grid['credit-suno']).toMatchObject({ w: 1, h: 1 })
  })

  it('works without a HOME size yet (first fit for everything)', () => {
    const grid = completeGrid({}, SHOWN, themeFrom, null)
    noOverlap(cellsOf(grid))
    expect(Object.keys(grid)).toHaveLength(SHOWN.length)
  })
})

describe("moving an older Castle's layout into the grid (W4)", () => {
  it('reads the old screenshot as overlapping', () => {
    const geo = geometry(FULL.area.width, 6)
    const free = Object.entries(OLD).map(([id, p]) => ({ id, ...cellFromOldPlace(id, p, FULL), x: ((p.left / 100) * FULL.home.width - FULL.area.left) / geo.colPitch, y: ((p.top / 100) * FULL.home.height - FULL.area.top) / geo.rowPitch }))
    expect(overlapping(free).length).toBeGreaterThan(0)
  })

  for (const [name, ctx] of WIDTHS) {
    it(`keeps every widget and leaves none overlapping (${name})`, () => {
      const ids = [...SHOWN]
      const grid = completeGrid({}, ids, oldFrom, ctx)
      const list = cellsOf(grid)
      expect(list.map((c) => c.id).sort()).toEqual([...ids].sort())
      noOverlap(list)
      inside(list, COLS)
      for (const [, at] of WIDTHS) noOverlap(layoutFor(list, { cols: columnsFor(at.area.width) }))
    })
  }

  it('keeps the old order: the clock above PROJECTS, USAGE and DIARY on the left', () => {
    const grid = completeGrid({}, Object.keys(OLD), oldFrom, FULL)
    expect(grid.clock.y).toBeLessThan(grid.projects.y)
    expect(grid.usage.x).toBe(0)
    expect(grid.diary.x).toBe(0)
    expect(grid.diary.y).toBeGreaterThan(grid.today.y)
  })

  it("turns the old L zoom into the nearest whole cells, never below the widget's minimum", () => {
    expect(cellFromOldPlace('today', { left: 50, top: 10 }, FULL, 'l')).toMatchObject({ w: 2, h: 2 })
    expect(cellFromOldPlace('today', { left: 50, top: 10 }, FULL, 's')).toMatchObject({ w: 2, h: 2 })
    expect(cellFromOldPlace('clock', { left: 99, top: 0 }, FULL)).toMatchObject({ x: 4, y: 0, w: 2 })
    expect(cellFromOldPlace('usage', { left: 0, top: 50 }, FULL).x).toBe(0)
  })
})

describe('a widget that appears later', () => {
  const base = completeGrid({}, SHOWN, themeFrom, FULL)

  it('never moves the cards already there', () => {
    const next = completeGrid(base, [...SHOWN, 'weather', 'credit-openai'], (id) => (id === 'weather' ? { theme: { left: '2%', bottom: '4%' } } : {}), FULL)
    for (const id of SHOWN) expect(next[id]).toEqual(base[id])
    noOverlap(cellsOf(next))
  })

  it('takes its theme spot when that is free', () => {
    const small = { clock: { x: 0, y: 0, w: 2, h: 1 } }
    const next = completeGrid(small, ['clock', 'gauges'], () => ({ theme: seed.widgets.gauges }), FULL)
    expect(next.gauges).toMatchObject({ y: 0, x: 4 })
  })

  it('keeps what it has and returns the same grid when nothing is missing', () => {
    expect(completeGrid(base, SHOWN, themeFrom, FULL)).toBe(base)
  })

  it('keeps the cells of a widget switched off, and packs it again when it comes back', () => {
    const without = SHOWN.filter((id) => id !== 'today')
    const moved = moveTo(layoutFor(cellsOf(base, without)), 'projects', base.today.x, base.today.y)
    const grid = { ...base, ...gridOf(moved) }
    expect(grid.today).toEqual(base.today)
    noOverlap(layoutFor(cellsOf(grid, SHOWN)))
  })
})

describe('moving and sizing with snap on', () => {
  const list = cellsOf(completeGrid({}, SHOWN, themeFrom, FULL))

  it('puts a dropped card on whole cells and moves the cards it hits down', () => {
    const next = moveTo(list, 'today', 0.4, 0.6)
    expect(next.find((c) => c.id === 'today')).toMatchObject({ x: 0, y: 1 })
    noOverlap(next)
    inside(next, COLS)
  })

  it('never lets a card leave HOME', () => {
    const next = moveTo(list, 'clock', 99, -5)
    expect(next.find((c) => c.id === 'clock')).toMatchObject({ x: 4, y: 0 })
    noOverlap(next)
  })

  it('every size chip and every custom size leaves no overlap', () => {
    for (const id of ['today', 'clock', 'calendar']) {
      for (const [w, h] of [[1, 1], [2, 1], [2, 2], [3, 3], [6, 2], [4, 5]]) {
        const next = resizeTo(list, id, w, h)
        noOverlap(next)
        inside(next, COLS)
        for (const [, at] of WIDTHS) noOverlap(layoutFor(next, { cols: columnsFor(at.area.width) }))
      }
    }
  })

  it("never goes below a widget's minimum and keeps a wider card on HOME", () => {
    expect(resizeTo(list, 'today', 1, 1).find((c) => c.id === 'today')).toMatchObject({ w: 2, h: 2 })
    const wide = resizeTo([{ id: 'clock', x: 5, y: 0, w: 1, h: 1 }], 'clock', 3, 3)[0]
    expect(wide).toMatchObject({ x: 3, w: 3, h: 3 })
    expect(resizeTo([], 'clock', 9, 99)[0]).toMatchObject({ w: 6, h: MAX_H })
  })

  it('places a sized widget that is not on HOME yet under everything else', () => {
    const next = resizeTo([{ id: 'clock', x: 0, y: 0, w: 2, h: 3 }], 'growth', 2, 1)
    expect(next.find((c) => c.id === 'growth')).toMatchObject({ x: 0, y: 3 })
  })

  it('settles a chain: a pushed card pushes the one below it', () => {
    const chain = [{ id: 'a', x: 0, y: 0, w: 2, h: 1 }, { id: 'b', x: 0, y: 1, w: 2, h: 1 }, { id: 'c', x: 0, y: 2, w: 2, h: 1 }, { id: 'd', x: 0, y: 1, w: 2, h: 2 }]
    const next = settle(chain, 'd')
    noOverlap(next)
    expect(next.find((c) => c.id === 'd')).toMatchObject({ y: 1 })
  })

  it('tidies cards up into gaps only when asked', () => {
    const gappy = [{ id: 'clock', x: 0, y: 0, w: 2, h: 1 }, { id: 'today', x: 0, y: 4, w: 2, h: 2 }, { id: 'diary', x: 2, y: 7, w: 2, h: 1 }]
    expect(pack(gappy).list.find((c) => c.id === 'today').y).toBe(4)
    const tidied = tidy(gappy)
    expect(tidied.find((c) => c.id === 'today').y).toBe(1)
    expect(tidied.find((c) => c.id === 'diary').y).toBe(0)
    noOverlap(tidied)
  })

  it('finds the first free spot in reading order', () => {
    const placed = [{ id: 'a', x: 0, y: 0, w: 4, h: 1 }, { id: 'b', x: 4, y: 0, w: 2, h: 2 }]
    expect(firstFit(placed, 2, 1)).toEqual({ x: 0, y: 1 })
    expect(firstFit(placed, 6, 1)).toEqual({ x: 0, y: 2 })
    expect(firstFit(placed, 2, 1, 4)).toEqual({ x: 0, y: 1 })
  })
})

describe('a narrower HOME', () => {
  const list = cellsOf(completeGrid({}, SHOWN, themeFrom, FULL))

  it('is worked out in reading order and never changes the full layout', () => {
    const half = reflow(list, 4)
    noOverlap(half)
    inside(half, 4)
    expect(cellsOf(gridOf(list))).toEqual(list)
    expect(reflow(list, 6)).toEqual(pack(list).list)
  })

  it('makes a card wider than the narrow HOME as wide as HOME', () => {
    const wide = [{ id: 'today', x: 0, y: 0, w: 3, h: 3 }, { id: 'clock', x: 3, y: 0, w: 3, h: 1 }]
    const two = reflow(wide, 2)
    expect(two.map((c) => c.w)).toEqual([2, 2])
    noOverlap(two)
  })
})

describe('snap off, and back on', () => {
  it('lets cards sit anywhere and any size (from one cell), overlapping', () => {
    const free = [{ id: 'today', x: 0, y: 0, w: 2, h: 2 }, { id: 'clock', x: 0, y: 0, w: 2, h: 1 }]
    const moved = moveTo(free, 'clock', 1.33, 0.52, { snap: false })
    expect(moved.find((c) => c.id === 'clock')).toMatchObject({ x: 1.35, y: 0.5 })
    expect(overlapping(moved)).toHaveLength(1)
    expect(resizeTo(free, 'today', 0.2, 3.37, { snap: false }).find((c) => c.id === 'today')).toMatchObject({ w: 1, h: 3.35 })
    expect(layoutFor(moved, { snap: false })).toEqual(moved)
  })

  it('switching snap on puts every card on its nearest cells and pushes the colliding ones down', () => {
    const grid = { today: { x: 0.2, y: 0.1, w: 2.3, h: 1.6 }, clock: { x: 1.4, y: 0.3, w: 1.6, h: 1 }, calendar: { x: 5.5, y: 3, w: 2, h: 2 } }
    const { patch, moved, pushed } = snapPatch(grid, true)
    expect(patch.widgetSnap).toBe(true)
    const list = cellsOf(patch.widgetGrid)
    noOverlap(list)
    inside(list, COLS)
    expect(moved).toBe(3)
    expect(pushed).toBe(1)
    expect(patch.widgetGrid.calendar).toEqual({ x: 4, y: 3, w: 2, h: 2 })
    expect(snapPatch(grid, false).patch).toEqual({ widgetSnap: false })
  })

  it('is on unless the member switched it off', () => {
    expect(snapOf({})).toBe(true)
    expect(snapOf({ widgetSnap: false })).toBe(false)
  })
})

describe('size choices', () => {
  it('snap on: the w×h chips, greyed below the minimum', () => {
    expect(sizeChoices('today', true).map((s) => [s.label, s.ok])).toEqual([['1×1', false], ['2×1', false], ['2×2', true], ['3×3', true]])
    expect(sizeChoices('clock', true).every((s) => s.ok)).toBe(true)
    expect(chosenSize('today', { w: 2, h: 2 }, true)).toBe('2x2')
    expect(chosenSize('today', { w: 2, h: 3 }, true)).toBe('')
  })

  it('snap off: S, M and L size the box (space, not a zoom)', () => {
    expect(sizeChoices('today', false).map((s) => [s.label, s.w, s.h])).toEqual([['S', 1.5, 1.5], ['M', 2, 2], ['L', 2.5, 2.5]])
    expect(chosenSize('clock', { w: 2.5, h: 1.25 }, false)).toBe('l')
  })

  it('knows a span for every widget', () => {
    expect(spanOf('credit-suno')).toMatchObject({ w: 1, h: 1 })
    expect(spanOf('unknown')).toMatchObject({ w: 2, h: 2 })
    expect(clampCell({ id: 'today', x: 'a', y: null, w: 1, h: 1 })).toEqual({ id: 'today', x: 0, y: 0, w: 2, h: 2 })
  })
})

describe("reading a theme's place", () => {
  it('turns percent, pixels and calc() into pixels', () => {
    expect(cssPx('12%', 1000)).toBe(120)
    expect(cssPx('190px', 1000)).toBe(190)
    expect(cssPx('calc(22% + 190px)', 1000)).toBe(410)
    expect(cssPx('calc(50% - 10px)', 1000)).toBe(490)
    expect(cssPx(null, 1000)).toBeNull()
    expect(cssPx('auto', 1000)).toBeNull()
  })

  it('reads right and bottom anchors from the far edges', () => {
    expect(cellFromTheme('gauges', { right: '2%', top: '4%' }, FULL)).toMatchObject({ x: 4, y: 0 })
    const growth = cellFromTheme('growth', { right: '2%', bottom: '4%', width: '28%' }, FULL)
    expect(growth.x).toBe(4)
    expect(growth.y).toBeGreaterThan(4)
    expect(cellFromTheme('clock', {}, FULL)).toBeNull()
    expect(cellFromTheme('clock', null, FULL)).toBeNull()
  })
})
