import { describe, expect, it } from 'vitest'
import { UNTICKED_FIRST, barParts, resolveLook, resultLine, sizeLabel, startingTicks, sweptLine, totalOf } from './sweep.js'

const groups = [
  { id: 'temp', bytes: 1_181_116_006 },
  { id: 'browser', bytes: 858_993_459 },
  { id: 'thumbs', bytes: 0 },
  { id: 'recycle', bytes: 4_294_967_296 },
]

describe('sweep', () => {
  it('says sizes the way people read them', () => {
    expect(sizeLabel(0)).toBe('0 B')
    expect(sizeLabel(512)).toBe('512 B')
    expect(sizeLabel(12 * 1024)).toBe('12 KB')
    expect(sizeLabel(850 * 1024 * 1024)).toBe('850 MB')
    expect(sizeLabel(6_871_947_673)).toBe('6.4 GB')
  })

  it('adds up the groups, or only the ticked ones', () => {
    expect(totalOf(groups)).toBe(1_181_116_006 + 858_993_459 + 4_294_967_296)
    expect(totalOf(groups, new Set(['temp']))).toBe(1_181_116_006)
  })

  it('splits the bar by share and leaves empty groups out', () => {
    const parts = barParts(groups)
    expect(parts.map((p) => p.id)).toEqual(['temp', 'browser', 'recycle'])
    expect(parts.reduce((s, p) => s + p.share, 0)).toBeCloseTo(1)
    expect(barParts([])).toEqual([])
  })

  it('says when it last swept', () => {
    const now = Date.UTC(2026, 10, 18, 12)
    expect(sweptLine(null, now)).toBe('Not swept yet')
    expect(sweptLine(now / 1000 - 30, now)).toBe('Last swept just now')
    expect(sweptLine(now / 1000 - 20 * 60, now)).toBe('Last swept 20 min ago')
    expect(sweptLine(now / 1000 - 5 * 3600, now)).toBe('Last swept 5 h ago')
    expect(sweptLine(now / 1000 - 3 * 86400, now)).toBe('Last swept 3 days ago')
  })

  it('shows the member look, then the theme look, then the bar', () => {
    expect(resolveLook('broom', 'ring')).toBe('broom')
    expect(resolveLook(null, 'ring')).toBe('ring')
    expect(resolveLook('sparkle', 'nope')).toBe('bar')
    expect(resolveLook(undefined, undefined)).toBe('bar')
  })

  it('starts the sheet with the Recycle Bin unticked unless the gear says so', () => {
    expect([...startingTicks(groups)]).toEqual(['temp', 'browser', 'thumbs'])
    expect(startingTicks(groups, true).has('recycle')).toBe(true)
  })

  it('opens the Rust build caches unticked, whatever the gear says', () => {
    const withRust = [...groups, { id: 'rust', bytes: 300 * 1024 ** 3 }]
    expect(UNTICKED_FIRST).toContain('rust')
    expect(startingTicks(withRust).has('rust')).toBe(false)
    expect(startingTicks(withRust, true).has('rust')).toBe(false)
    expect(startingTicks(withRust, true).has('recycle')).toBe(true)
    expect(startingTicks(withRust).has('temp')).toBe(true)
  })

  it('says what was freed and what stayed', () => {
    expect(resultLine([{ freed: 6_335_076_761, skipped: 200 }, { freed: 0, skipped: 14 }])).toBe('Freed 5.9 GB - 214 files skipped (in use)')
    expect(resultLine([{ freed: 1024 * 1024, skipped: 1 }])).toBe('Freed 1.0 MB - 1 file skipped (in use)')
    expect(resultLine([{ freed: 0, skipped: 0 }])).toBe('Freed 0 B')
  })
})
