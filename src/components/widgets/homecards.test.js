import { describe, expect, it } from 'vitest'
import { cardCount, cardItems } from './homecards.js'

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
