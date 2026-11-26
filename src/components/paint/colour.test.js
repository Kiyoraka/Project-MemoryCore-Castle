import { describe, expect, it } from 'vitest'
import { hexToHsv, hsvToHex, wheelColour, wheelPoint } from './colour.js'

describe("PAINT's colour wheel", () => {
  it('turns HSV into colours and back', () => {
    expect(hsvToHex(0, 1, 1)).toBe('#ff0000')
    expect(hsvToHex(120, 1, 1)).toBe('#00ff00')
    expect(hsvToHex(240, 1, 0.5)).toBe('#000080')
    expect(hsvToHex(0, 0, 1)).toBe('#ffffff')
    const back = hexToHsv('#00ff00')
    expect([Math.round(back.h), back.s, back.v]).toEqual([120, 1, 1])
  })

  it('reads the colour under a point, white in the middle, nothing outside', () => {
    expect(wheelColour(0, 0, 50)).toBe('#ffffff')
    expect(wheelColour(50, 0, 50)).toBe('#ff0000')
    expect(wheelColour(60, 0, 50)).toBeNull()
    expect(wheelColour(50, 0, 50, 0)).toBe('#000000')
  })

  it('finds where a colour sits', () => {
    const p = wheelPoint('#ff0000', 50)
    expect([Math.round(p.x), Math.round(p.y)]).toEqual([50, 0])
  })
})
