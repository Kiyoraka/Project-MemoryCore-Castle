import { describe, expect, it } from 'vitest'
import { readReveal, revealEdge, revealStyle } from './usagereveal.js'
import { builtInThemes, problemWith } from './loader.js'

const sky = readReveal({ lowUrl: 'low.jpg', mode: 'rise', angle: '32deg', from: 0.29, to: 0.85 })

describe('the picture fills with the week', () => {
  it('reads a rise and a fade, and refuses what it cannot use', () => {
    expect(sky).toEqual({ mode: 'rise', lowUrl: 'low.jpg', angle: '32deg', from: 0.29, to: 0.85 })
    expect(readReveal({ lowUrl: 'low.jpg', mode: 'fade' })).toEqual({ mode: 'fade', lowUrl: 'low.jpg' })
    expect(readReveal({ mode: 'fade' })).toBeNull() // no picture found
    expect(readReveal({ lowUrl: 'low.jpg', mode: 'spin' })).toBeNull()
    expect(readReveal(null)).toBeNull()
    expect(readReveal({ lowUrl: 'l', mode: 'rise', angle: 'up; background:red', from: 2, to: -1 })).toEqual({ mode: 'rise', lowUrl: 'l', angle: '0deg', from: 0, to: 1 })
  })

  it('rises with every percent, not in steps', () => {
    expect(revealStyle(0, sky).mask).toBe('linear-gradient(transparent, transparent)')
    expect(revealStyle(100, sky).mask).toBe('')
    expect(revealStyle(30, sky).mask).toBe('linear-gradient(32deg, #000 39.80%, transparent 45.80%)') // 0.29 + 0.56 * 0.3
    expect(revealStyle(31, sky).mask).not.toBe(revealStyle(30, sky).mask)
    expect(revealStyle(150, sky).mask).toBe('')
    expect(revealStyle(-5, sky).mask).toBe('linear-gradient(transparent, transparent)')
  })

  it('finds a low picture for every built-in theme', () => {
    for (const id of ['constellation', 'crystal', 'seed']) {
      const theme = builtInThemes().find((t) => t.id === id)
      const reveal = readReveal(theme.usageReveal)
      expect(reveal, id).not.toBeNull()
      expect(problemWith(theme), id).toBeNull()
    }
    expect(readReveal(builtInThemes().find((t) => t.id === 'constellation').usageReveal).mode).toBe('rise')
  })

  it('puts the gliding edge where the percent says', () => {
    expect(revealEdge(0, sky)).toBe(0)
    expect(revealEdge(30, sky)).toBeCloseTo(0.458, 3)
    expect(revealEdge(100, sky)).toBeGreaterThan(1)
  })

  it('fades in proportion', () => {
    const crystal = readReveal({ lowUrl: 'low.jpg', mode: 'fade' })
    expect(revealStyle(0, crystal)).toEqual({ opacity: 0 })
    expect(revealStyle(30, crystal)).toEqual({ opacity: 0.3 })
    expect(revealStyle(100, crystal)).toEqual({ opacity: 1 })
  })
})
