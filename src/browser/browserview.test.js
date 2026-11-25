import { describe, expect, it } from 'vitest'
import { ARTIFACTS, chipCount, dangerWords, dragWidths, fillLabel, groupOf, libraryTiles, moveSelection, newTabShortcuts, onScreen, shownAddress, signInHint, singleTabs, siteWarning, suggestionGroups, suggestionInput } from './browserview.js'

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

describe('the news on a tab', () => {
  it('shows the count the site gives, capped, and nothing without news', () => {
    expect(chipCount(3)).toBe('3')
    expect(chipCount(120)).toBe('99+')
    expect(chipCount(0)).toBe('')
    expect(chipCount(undefined)).toBe('')
  })
})

describe('the suggestions', () => {
  const rows = [
    { kind: 'page', text: 'Google Calendar', url: 'https://calendar.google.com/', sub: 'calendar.google.com' },
    { kind: 'search', text: 'google meet', url: 'https://duckduckgo.com/?q=google+meet', sub: '' },
    { kind: 'bookmark', text: 'Google Drive', url: 'https://drive.google.com/', sub: 'drive.google.com' },
  ]

  it('puts past searches on top as pills and sites below as cards, in one order', () => {
    const g = suggestionGroups(rows)
    expect(g.pills.map((r) => r.text)).toEqual(['google meet'])
    expect(g.cards.map((r) => r.text)).toEqual(['Google Calendar', 'Google Drive'])
    expect(g.order.map((r) => r.text)).toEqual(['google meet', 'Google Calendar', 'Google Drive'])
  })

  it('moves the selection with the arrows, through the typed text and around', () => {
    expect(moveSelection(-1, 'ArrowDown', 3)).toBe(0)
    expect(moveSelection(2, 'ArrowDown', 3)).toBe(-1)
    expect(moveSelection(-1, 'ArrowUp', 3)).toBe(2)
    expect(moveSelection(0, 'ArrowUp', 3)).toBe(-1)
    expect(moveSelection(1, 'a', 3)).toBe(1)
    expect(moveSelection(1, 'ArrowDown', 0)).toBe(-1)
  })

  it('types a search again as words and a site as its address', () => {
    expect(suggestionInput(rows[1])).toBe('google meet')
    expect(suggestionInput(rows[0])).toBe('https://calendar.google.com/')
    expect(suggestionInput(null)).toBe('')
  })
})

describe('pages side by side', () => {
  const list = [
    { id: 1, title: 'Threads', unread: 3 },
    { id: 2, title: 'Shopee', unread: 0 },
    { id: 3, title: 'YouTube', unread: 2 },
    { id: 4, title: 'New tab', unread: 0 },
  ]

  it('shows the group as one chip with its first page, how many more, and their news', () => {
    const g = groupOf({ list, active: 3, panes: [1, 3, 4], widths: [] })
    expect(g.label).toBe('Threads +2')
    expect(g.unread).toBe(5)
    expect(g.list).toBe('Threads\nYouTube\nNew tab')
    expect(g.on).toBe(true)
    expect(groupOf({ list, active: 1, panes: [], widths: [] })).toBe(null)
  })

  it('keeps the other tabs as single chips', () => {
    expect(singleTabs({ list, panes: [1, 3] }).map((t) => t.id)).toEqual([2, 4])
    expect(singleTabs({ list, panes: [] }).length).toBe(4)
  })

  it('knows what is on screen', () => {
    expect(onScreen({ list, active: 3, panes: [1, 3] })).toEqual([1, 3])
    expect(onScreen({ list, active: 2, panes: [1, 3] })).toEqual([2])
    expect(onScreen({ list, active: null, panes: [] })).toEqual([])
  })

  it('moves a grip between its two columns, neither below a tenth', () => {
    expect(dragWidths([0.5, 0.5], 0, 100, 1000)).toEqual([0.6, 0.4])
    const squeezed = dragWidths([0.5, 0.5], 0, 900, 1000)
    expect(squeezed[1]).toBeCloseTo(0.1)
    expect(dragWidths([0.5, 0.5], 1, 100, 1000)).toEqual([0.5, 0.5], 'no grip after the last column')
  })
})
