import { describe, expect, it } from 'vitest'
import { floodFill, rgbaOf } from './flood.js'

// a w x h picture from rows of characters: '.' white, '#' black, 'g' light grey
function picture(rows) {
  const h = rows.length
  const w = rows[0].length
  const data = new Uint8ClampedArray(w * h * 4)
  const colour = { '.': [255, 255, 255, 255], '#': [0, 0, 0, 255], g: [240, 240, 240, 255] }
  rows.forEach((row, y) => [...row].forEach((c, x) => data.set(colour[c], (y * w + x) * 4)))
  return { data, w, h }
}
const red = [255, 0, 0, 255]
const isRed = (p, x, y) => p.data[(y * p.w + x) * 4] === 255 && p.data[(y * p.w + x) * 4 + 1] === 0

describe("PAINT's bucket", () => {
  it('fills the area inside a closed line and stops at it', () => {
    const p = picture(['......', '.####.', '.#..#.', '.#..#.', '.####.', '......'])
    expect(floodFill(p.data, p.w, p.h, 2, 2, red)).toBe(4)
    expect(isRed(p, 3, 3)).toBe(true)
    expect(isRed(p, 0, 0)).toBe(false)
  })

  it('fills around a shape, through concave runs', () => {
    const p = picture(['.....', '.#.#.', '.#.#.', '.###.', '.....'])
    expect(floodFill(p.data, p.w, p.h, 0, 0, red)).toBe(25 - 7)
    expect(isRed(p, 2, 1)).toBe(true)
  })

  it('takes near colours in with a tolerance', () => {
    const p = picture(['..g', 'g..', '..#'])
    expect(floodFill(p.data, p.w, p.h, 0, 0, red, 0)).toBe(6)
    const q = picture(['..g', 'g..', '..#'])
    expect(floodFill(q.data, q.w, q.h, 0, 0, red, 20)).toBe(8)
  })

  it('does nothing outside the picture or on the same colour', () => {
    const p = picture(['..', '..'])
    expect(floodFill(p.data, p.w, p.h, -1, 0, red)).toBe(0)
    expect(floodFill(p.data, p.w, p.h, 0, 5, red)).toBe(0)
    expect(floodFill(p.data, p.w, p.h, 0, 0, [255, 255, 255, 255])).toBe(0)
    // a fill colour inside the tolerance still ends
    expect(floodFill(p.data, p.w, p.h, 0, 0, [250, 250, 250, 255], 30)).toBe(4)
  })

  it('reads the colour', () => {
    expect(rgbaOf('#ff8000')).toEqual([255, 128, 0, 255])
    expect(rgbaOf('#ff800080')).toEqual([255, 128, 0, 128])
    expect(rgbaOf('#fff')).toEqual([255, 255, 255, 255])
  })
})
