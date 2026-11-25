import { describe, expect, it } from 'vitest'
import { panelAfterPin } from './browserpanel.js'

describe("the BROWSER row's panel", () => {
  it('opens when BROWSER is pinned, and otherwise stays as the chevron left it', () => {
    expect(panelAfterPin(false, true, false)).toBe(true)
    expect(panelAfterPin(true, false, true)).toBe(true, 'unpinning leaves it open')
    expect(panelAfterPin(true, false, false)).toBe(false)
    expect(panelAfterPin(true, true, false)).toBe(false, 'no change of pin, no change')
  })
})
