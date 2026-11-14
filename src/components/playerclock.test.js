import { describe, expect, it } from 'vitest'
import { playerClock } from './playerclock.js'

describe('the small player clock', () => {
  it('shows minutes and padded seconds', () => {
    expect(playerClock(7.9)).toBe('0:07')
    expect(playerClock(40)).toBe('0:40')
    expect(playerClock(723)).toBe('12:03')
  })

  it('shows 0:00 for anything not yet usable', () => {
    for (const v of [0, -3, NaN, Infinity, undefined, null]) expect(playerClock(v)).toBe('0:00')
  })
})
