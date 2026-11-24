import { describe, expect, it, vi } from 'vitest'

const opened = { url: [], dev: [] }
vi.mock('../api/castle.js', () => ({ openUrl: vi.fn(async (u) => opened.url.push(u)) }))
vi.mock('../api/devgit.js', () => ({ devOpenLink: vi.fn(async (u) => opened.dev.push(u)) }))

const { linkWay, openLink } = await import('./links.js')

describe('which links open from a chat', () => {
  it('opens https pages in the browser', () => {
    expect(linkWay('https://github.com/Kiyoraka/Project-MemoryCore-Castle/pull/76')).toBe('web')
    expect(linkWay('https://claude.ai/artifact/abc')).toBe('web')
  })

  it('opens a server on this computer through the DEV opener', () => {
    expect(linkWay('http://localhost:5173/stations')).toBe('local')
    expect(linkWay('http://127.0.0.1:3000')).toBe('local')
  })

  it('never opens anything else', () => {
    for (const bad of ['http://example.com', 'javascript:alert(1)', 'mailto:a@b.c', 'file:///etc/passwd', 'https://a b', 'https://x.y/"q', `https://x.y/${'a'.repeat(600)}`, '', null]) {
      expect(linkWay(bad)).toBe(null)
    }
  })

  it('hands each kind to its own opener and refuses the rest', async () => {
    await openLink('https://x.y/a')
    await openLink('http://localhost:5173')
    expect(opened.url).toEqual(['https://x.y/a'])
    expect(opened.dev).toEqual(['http://localhost:5173'])
    await expect(openLink('javascript:alert(1)')).resolves.toBe(false)
  })
})
