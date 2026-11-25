import { describe, expect, it } from 'vitest'
import { ARTIFACTS, dangerWords, fillLabel, libraryTiles, newTabShortcuts, shownAddress, signInHint, siteWarning } from './browserview.js'

describe('the new-tab page', () => {
  const servers = [
    { port: 5173, folder: '/Users/w/Code/montee', program: 'node' },
    { port: 3000, folder: null, program: 'node' },
    { port: 5173, folder: '/Users/w/Code/montee', program: 'node' },
    { port: 3306, folder: null, program: 'mysqld' },
  ]

  it('shows Artifacts first, then each live web server once (no databases)', () => {
    expect(newTabShortcuts(servers, 'shortcuts')).toEqual([
      { name: 'Artifacts', url: ARTIFACTS, letter: 'A' },
      { name: 'montee :5173', url: 'http://localhost:5173', letter: 'M' },
      { name: 'Server :3000', url: 'http://localhost:3000', letter: 'S' },
    ])
  })

  it('shows nothing on a blank new tab, and copes with no servers', () => {
    expect(newTabShortcuts(servers, 'blank')).toEqual([])
    expect(newTabShortcuts(null, 'shortcuts')).toEqual([{ name: 'Artifacts', url: ARTIFACTS, letter: 'A' }])
  })
})

describe('the address bar', () => {
  it('shows the page in front, or nothing for a new tab', () => {
    expect(shownAddress({ list: [{ id: 2, url: 'https://x.y/' }], active: 2 })).toBe('https://x.y/')
    expect(shownAddress({ list: [{ id: 2, url: null }], active: 2 })).toBe('')
    expect(shownAddress(null)).toBe('')
  })
})

describe('the sign-in hint (F11)', () => {
  it('shows on the claude.ai sign-in page and Google accounts', () => {
    expect(signInHint('https://claude.ai/login?returnTo=%2Fcode%2Fartifacts')).toMatch(/email code/)
    expect(signInHint('https://accounts.google.com/v3/signin/challenge')).toMatch(/email code/)
  })

  it('stays away from other pages, new tabs and look-alike hosts', () => {
    expect(signInHint('https://claude.ai/code/artifacts')).toBe('')
    expect(signInHint('https://claude.ai.evil.example/login')).toBe('')
    expect(signInHint('http://claude.ai/login')).toBe('')
    expect(signInHint('')).toBe('')
    expect(signInHint(null)).toBe('')
  })
})

describe('the site check on the toolbar', () => {
  it('says what the Sentinel found, and nothing for a safe site', () => {
    expect(siteWarning({ verdict: 'dangerous', reason: 'it carries the name paypal but is not paypal\'s own site' })).toBe(
      "The Sentinel marked this site dangerous: it carries the name paypal but is not paypal's own site",
    )
    expect(siteWarning({ verdict: 'suspicious', reason: 'it is a bare address, not a named site' })).toMatch(/^The Sentinel marked this site suspicious: /)
    expect(siteWarning({ verdict: 'safe', reason: 'fine' })).toBe('')
    expect(siteWarning(null)).toBe('')
  })
})

describe('what came over from Chrome', () => {
  it('the new tab shows the most visited sites and the bookmarks as tiles', () => {
    const tiles = libraryTiles({ mostVisited: [{ host: 'example.org', url: 'https://example.org/', visits: 9 }], bookmarks: [{ title: '', url: 'https://docs.example.com/a', folder: '' }, { title: 'Board', url: 'https://b.example/', folder: 'Work' }] })
    expect(tiles.visited).toEqual([{ name: 'example.org', url: 'https://example.org/', letter: 'E' }])
    expect(tiles.marks.map((m) => m.name)).toEqual(['docs.example.com', 'Board'])
    expect(libraryTiles(null)).toEqual({ visited: [], marks: [] })
  })

  it('the Fill button names its user, or how many are saved', () => {
    expect(fillLabel([])).toBe('')
    expect(fillLabel([{ id: 1, host: 'example.org', username: 'ari' }])).toBe('Fill as ari')
    expect(fillLabel([{ id: 1, host: 'example.org', username: '' }])).toBe('Fill')
    expect(fillLabel([{ id: 1, username: 'a' }, { id: 2, username: 'b' }])).toBe('Fill (2 saved)')
  })
})

describe('the DANGER screen', () => {
  it('names the threat and lights the guards that caught it', () => {
    const words = dangerWords({ url: 'https://paypa1.com/', threat: 'phishing', reason: 'This site looks like a fake', caughtBy: ['lookalike', 'google'] })
    expect(words.threat).toBe('Phishing - this site copies a real sign-in page.')
    expect(words.address).toBe('https://paypa1.com/')
    expect(words.chips.filter((c) => c.lit).map((c) => c.name)).toEqual(['Google Safe Browsing', 'Look-alike'])
    expect(words.chips.map((c) => c.name)).toEqual(['Sentinel', 'SmartScreen', 'Google Safe Browsing', 'Look-alike'])
  })

  it("says Safari's warning on a Mac, and a mark with no kind is a dangerous site", () => {
    const words = dangerWords({ url: 'https://x.example/', threat: 'dangerous', reason: 'r', caughtBy: ['sentinel'] }, true)
    expect(words.chips[1].name).toBe("Safari's warning")
    expect(words.threat).toBe('Dangerous site - your companion marked it.')
    expect(words.chips[0].lit).toBe(true)
  })

  it('shows nothing when the page is not held back, and copes with an unknown threat', () => {
    expect(dangerWords(null)).toBe(null)
    expect(dangerWords({ threat: 'something-new' }).threat).toBe('Dangerous site - your companion marked it.')
  })
})
