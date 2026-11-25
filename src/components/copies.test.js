import { describe, expect, it } from 'vitest'
import { copiesChip, copiesLine } from './copies.js'

const real = 'C:\\Users\\Ari\\AppData\\Local\\MemoryCore Castle'
const left = 'C:\\Users\\Ari\\AppData\\Local\\Packages\\Claude_x\\LocalCache\\Local\\MemoryCore Castle\\memorycore-castle.exe'

describe('two copies of Castle', () => {
  it('points a leftover to the real one', () => {
    const line = copiesLine({ this: left, installed: real, leftover: true, two: true })
    expect(line).toMatch(/^This Castle is a leftover copy inside another app's private folder/)
    expect(line).toContain(`Open the real one in ${real}`)
    expect(line).toMatch(/not from inside another app\.$/)
    expect(copiesChip({ leftover: true, two: true })).toBe('Leftover copy')
  })

  it('names both copies when this one is not the installed one', () => {
    const line = copiesLine({ this: 'D:\\Castle\\memorycore-castle.exe', installed: real, leftover: false, two: true })
    expect(line).toBe(`Two copies of Castle are on this PC: this one (D:\\Castle\\memorycore-castle.exe) and the installed one (${real}). Updates go to the installed one.`)
    expect(copiesChip({ two: true })).toBe('Two copies')
  })

  it('says nothing for the one real copy', () => {
    expect(copiesLine({ this: `${real}\\memorycore-castle.exe`, installed: real, leftover: false, two: false })).toBe('')
    expect(copiesLine(null)).toBe('')
    expect(copiesChip(null)).toBe('')
  })
})
