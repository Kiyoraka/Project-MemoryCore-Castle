import { readFileSync } from 'node:fs'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRowStatus, LIT_MS, SAVED_MS } from './rowstatus.js'
import { SETTINGS_DEFAULTS, differsFromDefault, valueOf } from './settingsDefaults.js'
import { FIRST_PAGE, SETTINGS_GROUPS, SETTINGS_PAGES, pageOf, parseTarget, stepPage, subtitleOf } from './settingsnav.js'

describe('the sidebar map', () => {
  it('keeps the five groups in their fixed order', () => {
    expect(SETTINGS_GROUPS.map((g) => g.label)).toEqual(['You', 'Home', 'Connect', 'Data', 'App'])
  })

  it('still has every page Castle opens Settings at', () => {
    // App.vue, toasts and other rooms open these by id
    for (const id of ['companion', 'keys', 'database', 'widgets', 'sentinel', 'about', 'alerts', 'chat', 'rooms', 'quick', 'backup', 'startup']) {
      expect(SETTINGS_PAGES.some((p) => p.id === id)).toBe(true)
    }
  })

  it('has unique ids, short labels and one-line subtitles', () => {
    const ids = SETTINGS_PAGES.map((p) => p.id)
    expect(new Set(ids).size).toBe(ids.length)
    for (const page of SETTINGS_PAGES) {
      expect(page.label.split(' ').length).toBeLessThanOrEqual(5)
      expect(subtitleOf(page.id, 'Jerry').length).toBeLessThanOrEqual(90)
    }
  })

  it('names the companion in a subtitle', () => {
    expect(subtitleOf('chat', 'Jerry')).toBe('The CHAT room, and Jerry in it.')
    expect(pageOf('nowhere').id).toBe(FIRST_PAGE)
  })
})

describe('deep links', () => {
  it('reads a page and the row to scroll to', () => {
    expect(parseTarget('chat#pictures')).toEqual({ page: 'chat', anchor: 'pictures' })
    expect(parseTarget('keys')).toEqual({ page: 'keys', anchor: '' })
  })

  it('opens the first page for anything unknown', () => {
    expect(parseTarget('nowhere#x')).toEqual({ page: FIRST_PAGE, anchor: '' })
    expect(parseTarget(undefined)).toEqual({ page: FIRST_PAGE, anchor: '' })
    // a click event passed by mistake (openSettings used as a handler)
    expect(parseTarget({ type: 'click' })).toEqual({ page: FIRST_PAGE, anchor: '' })
  })
})

describe('moving through the sidebar with keys', () => {
  it('steps down and up, and wraps', () => {
    const last = SETTINGS_PAGES.at(-1).id
    expect(stepPage(FIRST_PAGE, 'ArrowDown')).toBe(SETTINGS_PAGES[1].id)
    expect(stepPage(FIRST_PAGE, 'ArrowUp')).toBe(last)
    expect(stepPage(last, 'ArrowDown')).toBe(FIRST_PAGE)
    expect(stepPage('keys', 'Home')).toBe(FIRST_PAGE)
    expect(stepPage('keys', 'End')).toBe(last)
    expect(stepPage('keys', 'a')).toBe(null)
  })
})

describe('defaults', () => {
  it('reads a missing value as its default', () => {
    expect(valueOf({}, 'dailyMoodOn')).toBe(true)
    expect(valueOf({ dailyMoodOn: null }, 'dailyMoodOn')).toBe(true)
    expect(valueOf({ dailyMoodOn: false }, 'dailyMoodOn')).toBe(false)
    expect(valueOf(null, 'roomVoicesPerDay')).toBe(2)
    expect(valueOf({ roomPicturesPerDay: 0 }, 'roomPicturesPerDay')).toBe(0)
  })

  it('opens SCRIPTORIUM in the classic layout until the member picks Studio', () => {
    expect(SETTINGS_DEFAULTS.scriptoriumLayout).toBe('classic')
    expect(valueOf({}, 'scriptoriumLayout')).toBe('classic')
    expect(valueOf({ scriptoriumLayout: 'studio' }, 'scriptoriumLayout')).toBe('studio')
    expect(differsFromDefault({ scriptoriumLayout: 'studio' }, 'scriptoriumLayout')).toBe(true)
  })

  it('knows when a value differs from its default', () => {
    expect(differsFromDefault({ clockFormat: '24h' }, 'clockFormat')).toBe(false)
    expect(differsFromDefault({ clockFormat: '12h' }, 'clockFormat')).toBe(true)
    expect(differsFromDefault({ anything: 1 }, 'anything')).toBe(false)
    expect(differsFromDefault({ automationsCommands: [] }, 'automationsCommands')).toBe(false)
    expect(differsFromDefault({ automationsCommands: ['program.exe'] }, 'automationsCommands')).toBe(true)
  })

  it("match the core's Settings::default", () => {
    // a Windows checkout has CRLF line ends; the block is found by plain newlines
    const rust = readFileSync(new URL('../../../src-tauri/src/settings.rs', import.meta.url), 'utf8').replace(/\r\n/g, '\n')
    const start = rust.indexOf('impl Default for Settings')
    const block = rust.slice(start, rust.indexOf('\n}\n', start))
    const camel = (snake) => snake.replace(/_([a-z])/g, (_, c) => c.toUpperCase())
    const core = {}
    for (const [, field, raw] of block.matchAll(/^\s+([a-z_]+): (.+),$/gm)) {
      const key = camel(field)
      if (raw === 'true' || raw === 'false') core[key] = raw === 'true'
      else if (/^\d+$/.test(raw)) core[key] = Number(raw)
      else if (/^"[^"]*"\.into\(\)$/.test(raw)) core[key] = raw.slice(1, raw.indexOf('"', 1))
      else if (raw === 'Cli::Claude') core[key] = 'claude'
      else if (raw === 'Vec::new()') core[key] = []
    }
    for (const [key, value] of Object.entries(SETTINGS_DEFAULTS)) {
      expect(core, key).toHaveProperty(key)
      expect(value, key).toEqual(core[key])
    }
  })
})

describe('row status', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('flashes Saved for a moment, then lets it go', async () => {
    const rows = createRowStatus()
    await rows.run('mood', async () => 'ok')
    expect(rows.state.saved.mood).toBe(true)
    vi.advanceTimersByTime(SAVED_MS)
    expect(rows.state.saved.mood).toBe(false)
  })

  it('keeps the reason under the row when a save fails, and clears it on the next try', async () => {
    const rows = createRowStatus()
    const result = await rows.run('mood', async () => {
      throw new Error('the disk is full')
    })
    expect(result).toBe(null)
    expect(rows.state.saved.mood).toBe(false)
    expect(rows.state.errors.mood).toBe('Not saved: the disk is full')
    await rows.run('mood', async () => 'ok')
    expect(rows.state.errors.mood).toBeUndefined()
    expect(rows.state.saved.mood).toBe(true)
  })

  it('says something even when the failure has no words', () => {
    const rows = createRowStatus()
    rows.failed('x', '')
    expect(rows.state.errors.x).toBe('Not saved. Try again.')
    rows.failed('y', 'select it by hand.', 'Not copied')
    expect(rows.state.errors.y).toBe('Not copied: select it by hand.')
  })

  it('lights a deep-linked row, then lets it go', () => {
    const rows = createRowStatus()
    rows.light('pictures')
    expect(rows.state.lit).toBe('pictures')
    vi.advanceTimersByTime(LIT_MS)
    expect(rows.state.lit).toBe('')
  })
})
