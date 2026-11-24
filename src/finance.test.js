import { describe, expect, it } from 'vitest'
import { amountText, byCurrency, categoryLabel, draftOf, dropLine, edited, money, monthTitle, shiftMonth } from './finance.js'

const receipt = { id: 4, shop: 'Kedai', totalCents: 1250, currency: 'MYR', spentOn: '2026-11-24', category: 'food', last4: '', note: '' }

describe('finance words and sums', () => {
  it('writes money with its currency', () => {
    expect(money(1250, 'MYR', 'en-US')).toBe('MYR 12.50')
    expect(money(123456, 'USD', 'en-US')).toBe('USD 1,234.56')
    expect(amountText(1205)).toBe('12.05')
    expect(amountText(7)).toBe('0.07')
    expect(categoryLabel('groceries')).toBe('Groceries')
  })

  it('never adds across currencies, and gives each category its share', () => {
    const groups = byCurrency([
      { currency: 'MYR', category: 'food', cents: 3000, count: 2 },
      { currency: 'USD', category: 'fun', cents: 500, count: 1 },
      { currency: 'MYR', category: 'bills', cents: 7000, count: 1 },
    ])
    expect(groups.map((g) => [g.currency, g.cents])).toEqual([['MYR', 10000], ['USD', 500]])
    expect(groups[0].rows.map((r) => [r.category, r.share])).toEqual([['bills', 0.7], ['food', 0.3]])
    expect(byCurrency()).toEqual([])
  })

  it('moves between months across a year', () => {
    expect(shiftMonth('2026-11', 1)).toBe('2026-12')
    expect(shiftMonth('2026-12', 1)).toBe('2027-01')
    expect(shiftMonth('2026-01', -1)).toBe('2025-12')
    expect(monthTitle('2026-11', 'en-US')).toBe('November 2026')
  })

  it('sends edits only when the member changed something', () => {
    const draft = draftOf(receipt)
    expect(draft).toEqual({ shop: 'Kedai', total: '12.50', currency: 'MYR', date: '2026-11-24', category: 'food', last4: '', note: '' })
    expect(edited(receipt, draft)).toBe(false)
    expect(edited(receipt, { ...draft, total: '13.50' })).toBe(true)
    expect(edited(receipt, { ...draft, shop: ' Kedai ' })).toBe(false)
  })

  it('answers a drop in one line', () => {
    expect(dropLine({ sent: 3, already: ['r1.jpg is already receipt #4 (kept)'], refused: [], left: 2 }, 'Alice')).toBe(
      'Sent 3 pictures to Alice; each waits here once read. r1.jpg is already receipt #4 (kept). Drop the other 2 after these are read.',
    )
    expect(dropLine({ sent: 0, already: [], refused: [], left: 0 }, 'Alice')).toBe('Nothing to send.')
    expect(dropLine(null, 'Alice')).toBe('')
  })
})
