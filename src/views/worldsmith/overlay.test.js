import { describe, expect, it } from 'vitest'
import { EDGE, SIZE_MAX, SIZE_MIN, TAIL, bubblePlace, fitSize, lensRing, newest, scaleBox, shownLines, textWidth } from './overlay.js'

describe('the Translate overlay', () => {
  it('scales a picture box onto the window', () => {
    expect(scaleBox({ x: 100, y: 50, w: 200, h: 20 }, 1000, 500, 2000, 1000)).toEqual({ x: 200, y: 100, w: 400, h: 40 })
    expect(scaleBox({ x: 10, y: 10, w: 10, h: 10 }, 0, 0, 800, 600)).toEqual({ x: 10, y: 10, w: 10, h: 10 })
    expect(scaleBox(null, 100, 100, 100, 100)).toEqual({ x: 0, y: 0, w: 0, h: 0 })
  })

  it('counts wide characters about twice a latin one', () => {
    expect(textWidth('あい', 20)).toBeCloseTo(40)
    expect(textWidth('ab', 20)).toBeLessThan(textWidth('あい', 20))
    expect(textWidth('', 20)).toBe(0)
  })

  it('fits text between 22 and 11 px', () => {
    expect(fitSize('Hi', 400, 2)).toBe(SIZE_MAX)
    const long = 'Welcome, traveller! Fresh potions, only 50 gold. '.repeat(3)
    const size = fitSize(long, 300, 3)
    expect(size).toBeLessThan(SIZE_MAX)
    expect(size).toBeGreaterThanOrEqual(SIZE_MIN)
    expect(fitSize('x'.repeat(5000), 100, 2)).toBe(SIZE_MIN)
    expect(fitSize('', 100, 2)).toBe(SIZE_MAX)
  })

  it('puts a bubble above its words with the tail down when there is room', () => {
    const box = { x: 400, y: 600, w: 200, h: 40 }
    const p = bubblePlace(box, 300, 80, 1600, 900)
    expect(p.tail).toBe('down')
    expect(p.top).toBe(600 - TAIL - 80)
    expect(p.left).toBe(350)
    expect(p.tailX).toBe(150)
  })

  it('puts a bubble below a box at the top edge, tail up', () => {
    const p = bubblePlace({ x: 400, y: 10, w: 200, h: 30 }, 300, 80, 1600, 900)
    expect(p.tail).toBe('up')
    expect(p.top).toBe(10 + 30 + TAIL)
  })

  it('clamps at the right edge and still points the tail at the words', () => {
    const box = { x: 1500, y: 500, w: 80, h: 30 }
    const p = bubblePlace(box, 400, 80, 1600, 900)
    expect(p.left).toBe(1600 - EDGE - 400)
    expect(p.left + p.tailX).toBeLessThanOrEqual(1600)
    expect(p.tailX).toBeGreaterThan(200)
    expect(p.tailX).toBeLessThanOrEqual(400 - 18)
  })

  it('clamps at the left edge too', () => {
    const p = bubblePlace({ x: 0, y: 500, w: 20, h: 20 }, 300, 60, 1600, 900)
    expect(p.left).toBe(EDGE)
    expect(p.tailX).toBe(18)
  })

  it('stays inside a tiny window', () => {
    const p = bubblePlace({ x: 20, y: 20, w: 40, h: 20 }, 300, 120, 200, 100)
    expect(p.left).toBe(0)
    expect(p.top).toBe(0)
    expect(['up', 'down']).toContain(p.tail)
    expect(Number.isFinite(p.tailX)).toBe(true)
  })

  it('picks the side with more room when neither fits', () => {
    const low = bubblePlace({ x: 100, y: 250, w: 50, h: 20 }, 100, 260, 400, 300)
    expect(low.tail).toBe('down')
    const high = bubblePlace({ x: 100, y: 20, w: 50, h: 20 }, 100, 260, 400, 300)
    expect(high.tail).toBe('up')
  })

  it('finds the newest line as the lowest one', () => {
    expect(newest([{ y: 100, h: 20 }, { y: 600, h: 40 }, { y: 50, h: 10 }])).toBe(1)
    expect(newest([{ y: 100, h: 20 }, { y: 100, h: 20 }])).toBe(1)
    expect(newest([])).toBe(-1)
    expect(newest(undefined)).toBe(-1)
  })

  it('draws the lens ring centred on its words', () => {
    const r = lensRing({ x: 100, y: 100, w: 200, h: 40 }, 1600, 900)
    expect(r.size).toBe(90)
    expect(r.left + r.size / 2).toBe(200)
    expect(r.top + r.size / 2).toBe(120)
    expect(lensRing({ x: 0, y: 0, w: 10, h: 500 }, 1600, 900).size).toBe(220)
  })

  it('shows only lines with a translation', () => {
    const lines = shownLines({ lines: [{ text: 'a', translated: 'A' }, { text: 'b', translated: ' ' }, { text: 'c', translated: 'C' }] })
    expect(lines.map((l) => l.index)).toEqual([0, 2])
    expect(shownLines(null)).toEqual([])
  })
})
