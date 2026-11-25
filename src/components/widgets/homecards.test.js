import { describe, expect, it, vi } from 'vitest'
import { cardCount, cardItems, ticker } from './homecards.js'

const reminders = (n) => Array.from({ length: n }, (_, i) => ({ title: `Reminder ${i + 1}` }))

describe('HOME cards list everything', () => {
  it('keeps every reminder and project (no cut at four)', () => {
    expect(cardItems(reminders(7))).toHaveLength(7)
    expect(cardItems(reminders(13)).at(-1).title).toBe('Reminder 13')
  })

  it('is empty for nothing', () => {
    expect(cardItems(null)).toEqual([])
    expect(cardItems(undefined)).toEqual([])
  })

  it('says how many in the header', () => {
    expect(cardCount('today', reminders(7))).toBe('7 open')
    expect(cardCount('projects', reminders(3))).toBe('3 active')
    expect(cardCount('today', reminders(1))).toBe('1 open')
  })

  it('says nothing when the card is empty or the kind has no count', () => {
    expect(cardCount('today', [])).toBe('')
    expect(cardCount('diary', reminders(2))).toBe('')
  })
})

describe('TODAY tick (F9)', () => {
  const setup = () => {
    const calls = []
    let settled = 0
    const t = ticker(async (id, done) => { calls.push([id, done]) }, () => { settled++ }, 6000)
    return { t, calls, settled: () => settled }
  }

  it('closes the reminder at once and settles after the wait', async () => {
    vi.useFakeTimers()
    const { t, calls, settled } = setup()
    await t.tick(4)
    expect(calls).toEqual([[4, true]])
    expect(t.has(4)).toBe(true)
    vi.advanceTimersByTime(6000)
    expect(t.has(4)).toBe(false)
    expect(settled()).toBe(1)
    vi.useRealTimers()
  })

  it('Undo opens it again and never settles twice', async () => {
    vi.useFakeTimers()
    const { t, calls, settled } = setup()
    await t.tick(4)
    await t.undo(4)
    expect(calls).toEqual([[4, true], [4, false]])
    vi.advanceTimersByTime(6000)
    expect(settled()).toBe(1)
    vi.useRealTimers()
  })

  it('a second tick, a reminder without an id, or a failed tick changes nothing', async () => {
    const { t, calls } = setup()
    await t.tick(null)
    expect(calls).toEqual([])
    const failing = ticker(async () => { throw new Error('no db') }, () => {})
    await expect(failing.tick(2)).rejects.toThrow('no db')
    expect(failing.has(2)).toBe(false)
  })
})
