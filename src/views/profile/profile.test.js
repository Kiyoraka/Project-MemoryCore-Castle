import { describe, expect, it } from 'vitest'
import { activeTraits, audienceOf, profileLookOf, starPositions, traitNote, waitingItems } from './profile.js'

const t = (number, name, status = 'active', extra = {}) => ({ number, name, line: `${name} line`, status, audience: 'everyone', origin: 'written', updatedAt: '', ...extra })

describe('the PROFILE tab', () => {
  it('keeps a known look and falls back to cards', () => {
    expect(profileLookOf('constellation')).toBe('constellation')
    expect(profileLookOf('spiral')).toBe('cards')
    expect(profileLookOf(undefined)).toBe('cards')
  })

  it('names who hears a trait, with everyone for anything unknown', () => {
    expect(audienceOf('owner').label).toBe('Only you')
    expect(audienceOf('never').label).toBe('Never in the room')
    expect(audienceOf('guests').id).toBe('everyone')
  })

  it('lists the traits she lives by number, never the waiting or retired ones', () => {
    const traits = [t(3, 'C'), t(1, 'A'), t(2, 'B', 'proposed'), t(4, 'D', 'retired')]
    expect(activeTraits(traits).map((x) => x.number)).toEqual([1, 3])
  })

  it('puts new proposals first in the waiting row, then the ones she proposed herself', () => {
    const items = waitingItems([{ name: 'Shy', line: 'flustered' }], [t(5, 'Curious', 'proposed'), t(1, 'A'), t(2, 'Warm', 'proposed')])
    expect(items.map((i) => [i.key, i.source, i.name])).toEqual([
      ['new-0', 'new', 'Shy'],
      ['trait-2', 'kept', 'Warm'],
      ['trait-5', 'kept', 'Curious'],
    ])
    expect(waitingItems(null, null)).toEqual([])
  })

  it('says when a trait changed and where it came from', () => {
    expect(traitNote(t(1, 'A', 'active', { updatedAt: '2026-11-14 20:15', origin: 'diary' }))).toBe('changed 2026-11-14 20:15 · From the diary')
    expect(traitNote(t(1, 'A', 'active', { origin: 'other' }))).toBe('')
  })
})

describe('the constellation', () => {
  const stars = [1, 2, 3, 4, 5, 6].map((n) => ({ key: `s${n}`, name: `Trait ${n}` })).concat([{ key: 'w1', name: 'New', waiting: true }])

  it('draws the same sky for the same traits', () => {
    expect(starPositions(stars, 600, 400)).toEqual(starPositions(stars, 600, 400))
  })

  it('keeps every star inside the sky and apart from the others', () => {
    const at = starPositions(stars, 600, 400)
    for (const p of at) {
      expect(p.x).toBeGreaterThan(0)
      expect(p.x).toBeLessThan(600)
      expect(p.y).toBeGreaterThan(0)
      expect(p.y).toBeLessThan(400)
    }
    for (let i = 0; i < at.length; i++) {
      for (let j = i + 1; j < at.length; j++) {
        expect(Math.hypot(at[i].x - at[j].x, at[i].y - at[j].y)).toBeGreaterThan(40)
      }
    }
  })

  it('handles an empty sky and a single star', () => {
    expect(starPositions([], 600, 400)).toEqual([])
    expect(starPositions([{ key: 'a', name: 'A' }], 600, 400)).toHaveLength(1)
  })
})
