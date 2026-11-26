import { describe, expect, it } from 'vitest'
import { KINDS, TOOLS, TOOL_KEY, available, lastTool, readFeatures, rememberTool, toolById, toolLabel } from './forge.js'

const memoryStorage = (start = {}) => {
  const data = { ...start }
  return { getItem: (k) => (k in data ? data[k] : null), setItem: (k, v) => (data[k] = String(v)), data }
}
const brokenStorage = {
  getItem() {
    throw new Error('blocked')
  },
  setItem() {
    throw new Error('blocked')
  },
}

describe('the Forge', () => {
  it('has three kinds and two tools, each of a known kind', () => {
    expect(Object.keys(KINDS)).toEqual(['hammer', 'lens', 'chisel'])
    expect(TOOLS.map((t) => t.id)).toEqual(['values', 'translate'])
    for (const t of TOOLS) expect(KINDS[t.kind]).toBeTruthy()
  })

  it('labels a tool by its kind in plain words', () => {
    expect(toolLabel(toolById('values'))).toBe('Hammer · live')
    expect(toolLabel(toolById('translate'))).toBe('Lens · screen')
    expect(toolLabel({ kind: 'nope' })).toBe('')
    expect(toolLabel(null)).toBe('')
  })

  it('says what a tool still needs from the helper', () => {
    const translate = toolById('translate')
    expect(available(translate, ['rect', 'thumb', 'x'])).toEqual({ ok: true, missing: [] })
    expect(available(translate, ['rect'])).toEqual({ ok: false, missing: ['thumb'] })
    expect(available(translate, undefined)).toEqual({ ok: false, missing: ['rect', 'thumb'] })
    expect(available(toolById('values'), [])).toEqual({ ok: true, missing: [] })
  })

  it('remembers the last tool, defaulting to Values', () => {
    const s = memoryStorage()
    expect(lastTool(s)).toBe('values')
    rememberTool(s, 'translate')
    expect(s.data[TOOL_KEY]).toBe('translate')
    expect(lastTool(s)).toBe('translate')
    rememberTool(s, 'unknown')
    expect(lastTool(s)).toBe('translate')
    expect(lastTool(memoryStorage({ [TOOL_KEY]: 'gone' }))).toBe('values')
  })

  it('survives a storage that throws or is missing', () => {
    expect(lastTool(brokenStorage)).toBe('values')
    expect(() => rememberTool(brokenStorage, 'translate')).not.toThrow()
    expect(lastTool(null)).toBe('values')
    expect(() => rememberTool(undefined, 'values')).not.toThrow()
  })

  it('reads caps features, an old helper giving none', () => {
    expect(readFeatures({ ok: true, features: ['rect', 'thumb'] })).toEqual(['rect', 'thumb'])
    expect(readFeatures({ ok: true })).toEqual([])
    expect(readFeatures(null)).toEqual([])
  })
})
