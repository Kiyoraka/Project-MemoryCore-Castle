import { describe, expect, it } from 'vitest'
import { clipPath, coverRect, glowBoxes, parsePosition, readGlow } from './quotaglow.js'
import { builtInThemes } from './loader.js'

describe('the light over a painted shining thing', () => {
  it('reads a background position the way CSS does', () => {
    expect(parsePosition()).toEqual([0.5, 0.5])
    expect(parsePosition('center')).toEqual([0.5, 0.5])
    expect(parsePosition('top')).toEqual([0.5, 0])
    expect(parsePosition('right')).toEqual([1, 0.5])
    expect(parsePosition('top left')).toEqual([0, 0])
    expect(parsePosition('center bottom')).toEqual([0.5, 1])
    expect(parsePosition('30% 70%')).toEqual([0.3, 0.7])
    expect(parsePosition('nonsense')).toEqual([0.5, 0.5])
  })

  it('finds where a cover wallpaper lands', () => {
    // a wide window crops the top and bottom of a 3:2 picture
    const wide = coverRect({ width: 1911, height: 1141 }, { width: 1536, height: 1024 }, 'center')
    expect(wide.width).toBeCloseTo(1911)
    expect(wide.x).toBeCloseTo(0)
    expect(wide.y).toBeCloseTo((1141 - 1274) / 2, 0)
    // a tall window crops the sides; "left" keeps the left edge
    const tall = coverRect({ width: 600, height: 1000 }, { width: 1536, height: 1024 }, 'left')
    expect(tall.height).toBeCloseTo(1000)
    expect(tall.x).toBeCloseTo(0)
    expect(coverRect({ width: 0, height: 10 }, { width: 1, height: 1 })).toBeNull()
  })

  it('refuses a glow that is not made of fractions', () => {
    expect(readGlow(null)).toBeNull()
    expect(readGlow({ x: 0.5, y: 0.5, width: 0.2 })).toBeNull()
    expect(readGlow({ x: 1.5, y: 0.5, width: 0.2, height: 0.2 })).toBeNull()
    expect(readGlow({ x: 0.5, y: 0.5, width: 0, height: 0.2 })).toBeNull()
    const g = readGlow({ x: 0.5, y: 0.5, width: 0.2, height: 0.4, outline: [[0, 0], [2, 1]], shine: { x: 0.5 } })
    expect(g.outline).toBeNull()
    expect(g.shine).toBeNull()
  })

  it('places the thing and its shine on screen', () => {
    const wall = { x: 0, y: -50, width: 1000, height: 600 }
    const b = glowBoxes(readGlow({ x: 0.5, y: 0.5, width: 0.2, height: 0.5, shine: { x: 0.5, y: 0.8 } }), wall)
    expect(b.thing).toEqual({ x: 400, y: 100, width: 200, height: 300 })
    expect(b.shine).toEqual({ x: 500, y: 430 })
    expect(glowBoxes(null, wall)).toBeNull()
  })

  it('cuts the outline as a clip path', () => {
    expect(clipPath(null)).toMatch(/^ellipse/)
    expect(clipPath([[0.5, 0], [1, 1], [0, 1]])).toBe('polygon(50.0% 0.0%, 100.0% 100.0%, 0.0% 100.0%)')
  })

  it('leaves the Crystal to its usage reveal (the crystal wakes as the week is used)', () => {
    const crystal = builtInThemes().find((t) => t.id === 'crystal')
    expect(readGlow(crystal.quotaGlow)).toBeNull()
    expect(crystal.usageReveal.mode).toBe('fade')
  })
})
