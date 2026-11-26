import { describe, expect, it } from 'vitest'
import { dragBegins, dropPlace, isOn, startsOnControl, memberPlace, originOf, placeFor, placePatch, resetPatch, adoptPatch, unplaced, scaleOf, sizeOf, sizePatch, switchPatch, WIDGETS, hasItsKey, roomIsOn , GRID_PX, snapOn, snapPatch, snapPx, packBoxes, arrangeWidgets, anyOverlap, arrangePatch, themeScaleOf } from './widgetplaces.js'

describe('HOME widgets as the member arranges them', () => {
  it('lists every widget the themes place', () => {
    expect(WIDGETS.map((w) => w.id)).toEqual([
      'clock', 'gauges', 'weather', 'disks', 'sentinel', 'automations', 'calendar', 'sweep', 'usage', 'today', 'projects', 'diary', 'growth', 'backup', 'database',
      'credit-openai', 'credit-seedance', 'credit-suno', 'credit-elevenlabs',
    ])
  })

  it('keeps an opt-in widget hidden until it is switched on', () => {
    expect(isOn('weather', {})).toBe(false)
    expect(isOn('weather', { widgetsOff: [] })).toBe(false)
    expect(isOn('weather', { widgetsOn: ['weather'] })).toBe(true)
    expect(switchPatch('weather', true, {})).toEqual({ widgetsOn: ['weather'] })
    expect(switchPatch('weather', false, { widgetsOn: ['weather'] })).toEqual({ widgetsOn: [] })
    expect(switchPatch('diary', false, {})).toEqual({ widgetsOff: ['diary'] })
  })

  it('shows a widget that belongs to a room only while that room is on', () => {
    expect(roomIsOn('automations', { automations: true })).toBe(true)
    expect(roomIsOn('automations', { automations: false })).toBe(false)
    expect(roomIsOn('automations', {})).toBe(true)
    expect(roomIsOn('disks', { automations: false })).toBe(true)
  })

  it('shows a credit widget only once its key is saved', () => {
    expect(hasItsKey('credit-suno', [])).toBe(false)
    expect(hasItsKey('credit-suno', ['suno', 'openai'])).toBe(true)
    expect(hasItsKey('diary', [])).toBe(true)
  })

  it('switches widgets on and off, USAGE through its own switch', () => {
    expect(isOn('diary', {})).toBe(true)
    expect(isOn('diary', { widgetsOff: ['diary'] })).toBe(false)
    expect(isOn('usage', { showUsageCard: false })).toBe(false)
    expect(isOn('usage', { widgetsOff: ['usage'] })).toBe(true)
    expect(switchPatch('usage', false)).toEqual({ showUsageCard: false })
    expect(switchPatch('diary', false, { widgetsOff: ['today'] })).toEqual({ widgetsOff: ['diary', 'today'] })
    expect(switchPatch('today', true, { widgetsOff: ['diary', 'today'] })).toEqual({ widgetsOff: ['diary'] })
  })

  it('keeps a size, medium by default and never stored', () => {
    expect(sizeOf('clock', {})).toBe('m')
    expect(sizeOf('clock', { widgetSizes: { clock: 'l' } })).toBe('l')
    expect(sizeOf('clock', { widgetSizes: { clock: 'xl' } })).toBe('m')
    expect(sizePatch('clock', 's', { widgetSizes: { today: 'l' } })).toEqual({ widgetSizes: { today: 'l', clock: 's' } })
    expect(sizePatch('today', 'm', { widgetSizes: { today: 'l' } })).toEqual({ widgetSizes: {} })
  })

  it("replaces the theme's anchors with the member's corner and keeps its width", () => {
    const theme = { right: '2%', top: 'calc(22% + 190px)', width: '28%' }
    expect(placeFor(theme, null)).toBe(theme)
    expect(placeFor(theme, { left: 40, top: 12.5 })).toEqual({ left: '40%', top: '12.5%', width: '28%' })
    expect(placeFor(undefined, { left: 1, top: 2 })).toEqual({ left: '1%', top: '2%' })
  })

  it('keeps a dropped widget wholly on HOME', () => {
    const home = { width: 1000, height: 500 }
    expect(dropPlace({ left: 250, top: 100, width: 200, height: 100 }, home)).toEqual({ left: 25, top: 20 })
    expect(dropPlace({ left: -40, top: -9, width: 200, height: 100 }, home)).toEqual({ left: 0, top: 0 })
    expect(dropPlace({ left: 990, top: 480, width: 200, height: 100 }, home)).toEqual({ left: 80, top: 80 })
    expect(dropPlace({ left: 1, top: 1, width: 1, height: 1 }, { width: 0, height: 0 })).toEqual({ left: 0, top: 0 })
  })

  it('keeps one arrangement for every theme and resets all of it', () => {
    let s = {}
    s = { ...s, ...placePatch(s, 'seed', 'today', { left: 50, top: 40 }) }
    s = { ...s, ...placePatch(s, 'crystal', 'clock', { left: 70, top: 5 }) }
    for (const theme of ['seed', 'crystal', 'sunset-shore']) {
      expect(memberPlace(s, theme, 'today')).toEqual({ left: 50, top: 40 })
      expect(memberPlace(s, theme, 'clock')).toEqual({ left: 70, top: 5 })
    }
    expect(resetPatch(s, 'seed')).toEqual({ widgetPlaces: {} })
  })

  it("brings a theme's older places along into the one arrangement", () => {
    const old = { widgetPlaces: { seed: { today: { left: 5, top: 6 } }, crystal: { today: { left: 9, top: 9 } } } }
    expect(memberPlace(old, 'seed', 'today')).toEqual({ left: 5, top: 6 })
    const next = placePatch(old, 'seed', 'clock', { left: 1, top: 2 })
    expect(next.widgetPlaces.all).toEqual({ today: { left: 5, top: 6 }, clock: { left: 1, top: 2 } })
    expect(memberPlace(next, 'crystal', 'today')).toEqual({ left: 5, top: 6 })
  })

  it('keeps a place for a widget that has none, once, and never moves one that has', () => {
    const s = { widgetPlaces: { all: { clock: { left: 70, top: 5 } } } }
    expect(unplaced(s, 'seed', ['clock', 'sweep'])).toEqual(['sweep'])
    const patch = adoptPatch(s, 'seed', { clock: { left: 1, top: 1 }, sweep: { left: 30, top: 40 } })
    expect(patch.widgetPlaces.all).toEqual({ clock: { left: 70, top: 5 }, sweep: { left: 30, top: 40 } })
    expect(adoptPatch({ widgetPlaces: patch.widgetPlaces }, 'seed', { clock: { left: 9, top: 9 }, sweep: { left: 9, top: 9 } })).toBeNull()
  })
})

describe('dragging a widget on HOME', () => {
  it('begins only past a few pixels, so a still press is a click', () => {
    expect(dragBegins(0, 0)).toBe(false)
    expect(dragBegins(3, 4)).toBe(false)
    expect(dragBegins(5, 5)).toBe(true)
    expect(dragBegins(-7, 0)).toBe(true)
  })
  it('never starts on a button, link or field', () => {
    expect(startsOnControl(['SPAN', 'BUTTON', 'DIV'])).toBe(true)
    expect(startsOnControl(['a'])).toBe(true)
    expect(startsOnControl(['DIV', 'P', 'SECTION'])).toBe(false)
    expect(startsOnControl()).toBe(false)
  })
})


describe('a theme that draws its widgets larger', () => {
  it("multiplies the member's own size by the theme's starting scale, kept within bounds", () => {
    expect(scaleOf('clock', {}, null)).toBe(1)
    expect(scaleOf('clock', {}, { widgetScale: 1.3 })).toBe(1.3)
    expect(scaleOf('clock', { widgetSizes: { clock: 'l' } }, { widgetScale: 1.3 })).toBe(1.495)
    expect(scaleOf('clock', { widgetSizes: { clock: 's' } }, {})).toBe(0.85)
    expect(scaleOf('clock', {}, { widgetScale: 9 })).toBe(1.6)
    expect(scaleOf('clock', {}, { widgetScale: 0.1 })).toBe(0.75)
    expect(scaleOf('clock', {}, { widgetScale: 'big' })).toBe(1)
  })

  it('grows each widget from the corner it is anchored to', () => {
    expect(originOf({ right: '2%', top: '4%' })).toBe('right top')
    expect(originOf({ left: '35%', bottom: '10%' })).toBe('left bottom')
    expect(originOf({ right: '2%', bottom: '8%' })).toBe('right bottom')
    expect(originOf({ left: '10%', top: '20%' })).toBe('left top')
    expect(originOf({})).toBe('left top')
  })
})

describe('HOME widgets snapped to a grid and arranged for the member', () => {
  const overlapsAny = (places, sizes) => {
    const boxes = Object.entries(places).map(([id, p]) => ({ ...p, width: sizes[id].width, height: sizes[id].height }))
    return anyOverlap(boxes)
  }

  it('snaps on unless the member turned it off', () => {
    expect(snapOn({})).toBe(true)
    expect(snapOn({ widgetSnap: false })).toBe(false)
    expect(snapPatch(false)).toEqual({ widgetSnap: false })
    expect(snapPx(23)).toBe(GRID_PX)
    expect(snapPx(25)).toBe(GRID_PX * 2)
    expect(snapPx(25, 0)).toBe(25)
  })

  it('puts a dropped widget on the grid and still wholly on HOME', () => {
    const home = { width: 1600, height: 1000 }
    expect(dropPlace({ left: 103, top: 57, width: 200, height: 100 }, home, 16)).toEqual({ left: 6, top: 6.4 })
    expect(dropPlace({ left: 103, top: 57, width: 200, height: 100 }, home)).toEqual({ left: 6.4, top: 5.7 })
    expect(dropPlace({ left: 1590, top: 995, width: 200, height: 100 }, home, 16)).toEqual({ left: 87.5, top: 90 })
  })

  it('lays widgets out so none overlaps another or the panel', () => {
    const area = { left: 16, top: 16, right: 1584, bottom: 984 }
    const panel = { left: 16, top: 16, width: 480, height: 968 }
    const sizes = { a: { width: 300, height: 200 }, b: { width: 300, height: 200 }, c: { width: 450, height: 120 }, d: { width: 200, height: 300 } }
    const boxes = Object.entries(sizes).map(([id, s]) => ({ id, ...s }))
    const { places, left } = packBoxes(boxes, area, [panel], { gap: 16, grid: 16 })
    expect(left).toEqual([])
    expect(overlapsAny(places, sizes)).toBe(false)
    for (const [id, p] of Object.entries(places)) {
      expect(anyOverlap([{ ...p, ...sizes[id] }, panel])).toBe(false)
      expect(p.left % 16).toBe(0)
      expect(p.top % 16).toBe(0)
      expect(p.left + sizes[id].width).toBeLessThanOrEqual(area.right)
    }
    // the first widget takes the top spot next to the panel
    expect(places.a).toEqual({ left: 512, top: 16 })
  })

  it('catches an overlap (so the check above can fail)', () => {
    expect(anyOverlap([{ left: 0, top: 0, width: 100, height: 100 }, { left: 99, top: 99, width: 10, height: 10 }])).toBe(true)
    expect(anyOverlap([{ left: 0, top: 0, width: 100, height: 100 }, { left: 100, top: 0, width: 10, height: 10 }])).toBe(false)
  })

  it('makes widgets smaller only when they would not fit otherwise', () => {
    const area = { left: 0, top: 0, right: 1000, bottom: 600 }
    const unit = { width: 340, height: 300 }
    // six M widgets of 340x300 fit only two to a row and one row in 1000x600; at S (289x255) all six do
    const items = ['a', 'b', 'c', 'd', 'e', 'f'].map((id) => ({ id, size: 'm', unit }))
    const done = arrangeWidgets(items, area, [], { gap: 16 })
    expect(done.left).toEqual([])
    expect(Object.values(done.sizes).every((s) => s === 's')).toBe(true)
    expect(done.shrunk).toHaveLength(6)
    // two fit as they are: nothing is changed
    const easy = arrangeWidgets(items.slice(0, 2), area, [], { gap: 16 })
    expect(easy.shrunk).toEqual([])
    expect(easy.sizes).toEqual({ a: 'm', b: 'm' })
  })

  it('says which widgets found no room, and keeps their size', () => {
    const area = { left: 0, top: 0, right: 400, bottom: 300 }
    const items = [{ id: 'big', size: 'l', unit: { width: 380, height: 280 } }, { id: 'more', size: 'm', unit: { width: 380, height: 280 } }]
    const done = arrangeWidgets(items, area, [], { gap: 16 })
    // made M, BIG would fit but MORE would not: one is left out either way, so nothing is made smaller
    expect(done.left).toEqual(['big'])
    expect(done.shrunk).toEqual([])
    expect(done.sizes).toEqual({ big: 'l', more: 'm' })
    expect(done.places.more).toEqual({ left: 0, top: 0 })
  })

  it('tries a tighter order before making anything smaller', () => {
    // in reading order the last wide one finds no room; another order fits all four at their own size
    const area = { left: 0, top: 0, right: 600, bottom: 400 }
    const items = [[250, 90], [130, 360], [140, 290], [370, 90]].map(([width, height], i) => ({ id: `w${i}`, size: 'm', unit: { width, height } }))
    const inOrder = packBoxes(items.map((it) => ({ id: it.id, ...it.unit })), area, [], { gap: 16 })
    expect(inOrder.left).toEqual(['w3'])
    const done = arrangeWidgets(items, area, [], { gap: 16 })
    expect(done.left).toEqual([])
    expect(done.shrunk).toEqual([])
  })

  it('saves an arrangement as places in percent and sizes, for every theme', () => {
    const settings = { widgetPlaces: { all: { clock: { left: 50, top: 5 } } }, widgetSizes: { diary: 'l' } }
    const patch = arrangePatch(settings, 'seed', { diary: { left: 400, top: 100 } }, { width: 1600, height: 1000 }, { diary: 's' })
    expect(patch.widgetPlaces.all).toEqual({ clock: { left: 50, top: 5 }, diary: { left: 25, top: 10 } })
    expect(patch.widgetSizes).toEqual({ diary: 's' })
    const backToM = arrangePatch(settings, 'seed', {}, { width: 1600, height: 1000 }, { diary: 'm' })
    expect(backToM.widgetSizes).toEqual({})
  })

  it('keeps the theme scale on its own', () => {
    expect(themeScaleOf(null)).toBe(1)
    expect(themeScaleOf({ widgetScale: 1.3 })).toBe(1.3)
    expect(themeScaleOf({ widgetScale: 9 })).toBe(1.6)
  })
})
