import { beforeEach, describe, expect, it, vi } from 'vitest'

const calls = []
let answer = 'castle'
vi.mock('../api/castle.js', () => ({ linkOpen: vi.fn(async (u) => { calls.push(u); return answer }) }))

const { LINK_MENU, linkMenus, linkWay, openLink } = await import('./links.js')

describe('which links open from a chat', () => {
  beforeEach(() => { calls.length = 0; answer = 'castle' })

  it('opens https pages and servers on this computer', () => {
    expect(linkWay('https://github.com/Kiyoraka/Project-MemoryCore-Castle/pull/76')).toBe('web')
    expect(linkWay('https://claude.ai/artifact/abc')).toBe('web')
    expect(linkWay('http://localhost:5173/stations')).toBe('local')
    expect(linkWay('http://127.0.0.1:3000')).toBe('local')
  })

  it('never opens anything else', () => {
    for (const bad of ['http://example.com', 'javascript:alert(1)', 'mailto:a@b.c', 'file:///etc/passwd', 'https://a b', 'https://x.y/"q', `https://x.y/${'a'.repeat(600)}`, '', null]) {
      expect(linkWay(bad)).toBe(null)
    }
  })

  it('asks the core, which follows Settings (Castle browser or the member\'s own)', async () => {
    await expect(openLink('https://x.y/a')).resolves.toBe('castle')
    expect(calls).toEqual(['https://x.y/a'])
  })

  it('shows the link menu where the click was when Settings say ask', async () => {
    answer = 'ask'
    const shown = []
    const listener = (e) => shown.push(e.detail)
    linkMenus.addEventListener(LINK_MENU, listener)
    await expect(openLink('https://x.y/b', { x: 10, y: 20 })).resolves.toBe('ask')
    linkMenus.removeEventListener(LINK_MENU, listener)
    expect(shown).toEqual([{ url: 'https://x.y/b', x: 10, y: 20 }])
  })

  it('refuses a link that may not open, without asking the core', async () => {
    await expect(openLink('javascript:alert(1)')).resolves.toBe(false)
    expect(calls).toEqual([])
  })
})
