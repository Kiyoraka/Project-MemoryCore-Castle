import { describe, expect, it } from 'vitest'
import { chooseLine, isDesignGroup } from './designpick.js'

const d = (n) => ({ path: `D:\\Shop\\Project Resources\\Design\\login\\login-${n}.png` })

describe('choosing designs in the chat', () => {
  it('knows a design group: two to four pictures from one Design folder', () => {
    expect(isDesignGroup([d(1), d(2), d(3)])).toBe(true)
    expect(isDesignGroup(['C:/x/design/a.png', 'C:/x/design/b.png'])).toBe(true)
    expect(isDesignGroup([d(1)])).toBe(false)
    expect(isDesignGroup([d(1), d(2), d(3), d(4), d(5)])).toBe(false)
    expect(isDesignGroup([d(1), { path: 'D:\\Shop\\Project Resources\\Design\\other\\x.png' }])).toBe(false)
    expect(isDesignGroup([{ path: 'C:\\Pictures\\a.png' }, { path: 'C:\\Pictures\\b.png' }])).toBe(false)
    expect(isDesignGroup(null)).toBe(false)
  })

  it('sends the Keep\'s words for one design, then a plain sentence', () => {
    const line = chooseLine([d(1), d(2), d(3)], [d(2).path])
    expect(line.startsWith('choose design login-2.png - I choose login-2.png.')).toBe(true)
    expect(line).toContain('build from it')
  })

  it('sends several in picture order and asks for looks in Settings', () => {
    const line = chooseLine([d(1), d(2), d(3)], [d(3).path, d(1).path])
    expect(line.startsWith('choose design login-1.png, login-3.png - I choose login-1.png and login-3.png.')).toBe(true)
    expect(line).toContain('looks I can switch in Settings')
    expect(chooseLine([d(1), d(2)], [])).toBe('')
  })
})
