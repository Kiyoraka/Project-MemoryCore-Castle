import { describe, expect, it } from 'vitest'
import { dragBegins, dropPlace, isOn, startsOnControl, memberPlace, originOf, placeFor, placePatch, resetPatch, adoptPatch, unplaced, scaleOf, sizeOf, sizePatch, switchPatch, WIDGETS, hasItsKey, roomIsOn } from './widgetplaces.js'

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
