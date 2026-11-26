import { describe, expect, it } from 'vitest'
import { TOTAL_MS, strokePlan } from './strokeanim.js'

describe("the companion's strokes drawing themselves", () => {
  it('draws every path in order, within 2.4 seconds, fills after', () => {
    const plan = strokePlan([100, 300, 600])
    expect(plan.steps).toHaveLength(3)
    plan.steps.forEach((s, i) => {
      if (i) expect(s.delay).toBeGreaterThanOrEqual(plan.steps[i - 1].delay + plan.steps[i - 1].duration - 1)
      expect(s.delay + s.duration).toBeLessThanOrEqual(TOTAL_MS)
    })
    expect(plan.steps[2].duration).toBeGreaterThan(plan.steps[0].duration)
    expect(plan.fillDelay).toBeLessThanOrEqual(TOTAL_MS)
    expect(plan.fillDelay).toBeGreaterThanOrEqual(plan.steps[2].delay)
  })

  it('gives a short path its moment', () => {
    expect(strokePlan([1, 100000]).steps[0].duration).toBeGreaterThanOrEqual(80)
  })

  it('shares evenly when there are very many paths or no lengths', () => {
    const many = strokePlan(Array(1000).fill(5))
    const last = many.steps.at(-1)
    expect(last.delay + last.duration).toBeLessThanOrEqual(TOTAL_MS)
    expect(strokePlan([0, NaN, -3]).steps.map((s) => s.duration)).toEqual([800, 800, 800])
  })

  it('has nothing to draw for no paths', () => {
    expect(strokePlan([])).toEqual({ steps: [], fillDelay: 0, fillDuration: 500 })
  })
})
