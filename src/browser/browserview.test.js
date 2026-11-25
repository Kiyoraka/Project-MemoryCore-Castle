import { describe, expect, it } from 'vitest'
import { barOf, besideSheet, boundsOf, chipTitle, columnsOf, filterLogins } from './browserview.js'
import { bytesWords, dlCard, dlFilter, dlRing, isVideoLink, timeLeft } from './browserview.js'
import { ARTIFACTS, chipCount, dangerWords, dragWidths, fillLabel, groupOf, languageLabel, libraryTiles, moveSelection, newTabShortcuts, onScreen, sameLanguage, showTranslateRow, shownAddress, signInHint, singleTabs, siteOf, siteWarning, suggestionGroups, suggestionInput } from './browserview.js'

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

describe('the translate row', () => {
  const languages = [
    ['en', 'English'],
    ['ja', 'Japanese'],
    ['zh-CN', 'Chinese (Simplified)'],
    ['zh-TW', 'Chinese (Traditional)'],
    ['ms', 'Malay'],
  ]

  it("names a page's language from its own tag", () => {
    expect(languageLabel('ja-jp', languages)).toBe('Japanese')
    expect(languageLabel('zh-tw', languages)).toBe('Chinese (Traditional)')
    expect(languageLabel('ms', languages)).toBe('Malay')
    expect(languageLabel('', languages)).toBe('This page')
    expect(languageLabel('xx', languages)).toBe('XX')
  })

  it("shows for another language, a translation in progress, or when asked, never on a DANGER screen", () => {
    expect(showTranslateRow({ url: 'https://a/', lang: 'ja' }, 'en')).toBe(true)
    expect(showTranslateRow({ url: 'https://a/', lang: 'en-us' }, 'en')).toBe(false)
    expect(showTranslateRow({ url: 'https://a/', lang: '' }, 'en')).toBe(false)
    expect(showTranslateRow({ url: 'https://a/', lang: '' }, 'en', true)).toBe(true)
    expect(showTranslateRow({ url: 'https://a/', lang: 'en', translated: 'ms' }, 'en')).toBe(true)
    expect(showTranslateRow({ url: 'https://a/', lang: 'ja', danger: {} }, 'en')).toBe(false)
    expect(showTranslateRow({ url: null, lang: 'ja' }, 'en')).toBe(false)
    expect(sameLanguage('zh-tw', 'zh-CN')).toBe(false)
  })

  it('reads the site for Always', () => {
    expect(siteOf('https://ncode.syosetu.com/n1/2/')).toBe('ncode.syosetu.com')
    expect(siteOf('not a url')).toBe('')
  })
})


describe("the Keep's look", () => {
  it('cuts a chip title to 28 characters', () => {
    expect(chipTitle('Short')).toBe('Short')
    expect(chipTitle('x'.repeat(40))).toBe(`${'x'.repeat(27)}…`)
    expect(chipTitle(null)).toBe('')
  })
  it('lays the panes out with a grip between two', () => {
    expect(columnsOf([])).toBe('1fr')
    expect(columnsOf([0.5, 0.5])).toBe('0.5fr 10px 0.5fr')
    expect(columnsOf([0.25, 0.25, 0.25, 0.25]).split(' 10px ').length).toBe(4)
  })
  it('gives the core whole-pixel boxes, never a sliver', () => {
    expect(boundsOf({ left: 10.4, top: 120.6, width: 600.2, height: 500 })).toEqual({ x: 10, y: 121, w: 600, h: 500 })
    expect(boundsOf({ left: 0, top: 0, width: 1, height: 400 })).toBe(null)
    expect(boundsOf(null)).toBe(null)
  })
  it('keeps each column at least 15 percent when asked', () => {
    expect(dragWidths([0.5, 0.5], 0, 900, 1000, 0.15)[1]).toBeCloseTo(0.15)
  })
})

describe('the Passwords sheet', () => {
  it('finds a login by its site or its username', () => {
    const logins = [{ id: 1, host: 'github.com', username: 'ari' }, { id: 2, host: 'shop.example', username: 'Kiyo' }]
    expect(filterLogins(logins, 'git').map((l) => l.id)).toEqual([1])
    expect(filterLogins(logins, 'kiyo').map((l) => l.id)).toEqual([2])
    expect(filterLogins(logins, '  ').length).toBe(2)
  })
})

describe('the bookmarks bar', () => {
  it('puts bar bookmarks as pills and the rest in their folders', () => {
    const bar = barOf([
      { title: 'GitHub', url: 'https://github.com/', folder: 'Bookmarks bar' },
      { title: 'Star', url: 'https://star.example/', folder: '' },
      { title: 'Docs', url: 'https://docs.example/', folder: 'Bookmarks bar/Work' },
      { title: 'Deep', url: 'https://deep.example/', folder: 'Bookmarks bar/Work/Specs' },
      { title: 'Recipe', url: 'https://food.example/', folder: 'Other bookmarks/Food' },
    ])
    expect(bar.sites.map((s) => s.title)).toEqual(['GitHub', 'Star'])
    expect(bar.folders.map((f) => f.name)).toEqual(['Work', 'Other bookmarks'])
    expect(bar.folders[0].items.map((i) => i.sub)).toEqual(['', 'Specs'])
    expect(bar.folders[1].items[0].sub).toBe('Food')
  })
})

describe('the downloader', () => {
  it('says sizes and time left in his words', () => {
    expect(bytesWords(512)).toBe('512 B')
    expect(bytesWords(12.4 * 1024 * 1024)).toBe('12.4 MB')
    expect(bytesWords(1.8 * 1024 ** 3)).toBe('1.8 GB')
    expect(timeLeft(0, 120 * 1024 * 1024, 1024 * 1024)).toBe('2 min left')
    expect(timeLeft(10, 0, 5)).toBe('')
  })

  it('draws each state as its card says', () => {
    const run = dlCard({ state: 'running', size: 1000, done: 620, parts: 8, speed: 13002342, category: 'Video' })
    expect(run.colour).toBe('mint')
    expect(run.percent).toBe(62)
    expect(run.icon).toBe('film')
    expect(run.line).toMatch(/^8 parts · 12\.4 MB\/s/)
    expect(dlCard({ state: 'paused', size: 100, done: 35 })).toMatchObject({ colour: 'amber', line: 'Paused at 35%' })
    expect(dlCard({ state: 'done', size: 2048, category: 'Programs' })).toMatchObject({ colour: 'gold', percent: 100, line: '2.0 KB', icon: 'box' })
    expect(dlCard({ state: 'failed', line: 'This address is dangerous - it is not downloaded.' })).toMatchObject({ colour: 'coral', line: 'This address is dangerous - it is not downloaded.' })
    expect(dlCard({ state: 'queued' }).line).toBe('Queued')
  })

  it('filters by category, knows a video link, and rings the chip', () => {
    const items = [{ category: 'Video', state: 'running', size: 100, done: 50 }, { category: 'Music', state: 'running', size: 100, done: 100 }, { category: 'Documents', state: 'done' }]
    expect(dlFilter(items, 'All').length).toBe(3)
    expect(dlFilter(items, 'Music').length).toBe(1)
    expect(isVideoLink('https://www.youtube.com/watch?v=abc')).toBe(true)
    expect(isVideoLink('https://youtu.be/abc')).toBe(true)
    expect(isVideoLink('https://www.threads.com/@x/post/1')).toBe(true)
    expect(isVideoLink('https://releases.ubuntu.com/ubuntu.iso')).toBe(false)
    expect(dlRing(items)).toBe(75)
    expect(dlRing([{ state: 'done' }])).toBe(null)
  })
})

describe('a side panel beside the pages', () => {
  it('ends a page where the panel begins, and drops one left with nothing', () => {
    const box = { x: 20, y: 100, w: 1800, h: 900 }
    expect(besideSheet(box, { left: 1300 })).toEqual({ x: 20, y: 100, w: 1272, h: 900 })
    expect(besideSheet({ x: 1400, y: 100, w: 400, h: 900 }, { left: 1300 })).toBe(null)
    expect(besideSheet(box, null)).toBe(box)
    expect(besideSheet({ x: 20, y: 100, w: 500, h: 900 }, { left: 1300 })).toEqual({ x: 20, y: 100, w: 500, h: 900 })
  })
})

import { DRAG_PX, HOME, atHome, dragPast, ghostLine, homeView, nearEdge, outChips } from './browserview.js'

describe('a tab in a window of its own: the drag and the out chips', () => {
  const list = [
    { id: 1, title: 'Gmail', url: 'https://mail.example/', unread: 4, window: 'home' },
    { id: 2, title: 'Threads', url: 'https://threads.example/', unread: 3, window: 'browser-out-1' },
    { id: 3, title: 'Video', url: 'https://video.example/', unread: 0, window: 'browser-out-2' },
    { id: 4, title: 'Secret', url: 'https://secret.example/', unread: 1, private: true, window: 'browser-out-2', danger: { url: 'x' } },
    { id: 5, title: 'Old tab', url: null },
  ]

  it('lifts a chip only past eight pixels, so a click stays a click', () => {
    expect(DRAG_PX).toBe(8)
    const down = { x: 100, y: 20 }
    expect(dragPast(down, { x: 105, y: 24 })).toBe(false)
    expect(dragPast(down, { x: 108, y: 20 })).toBe(false)
    expect(dragPast(down, { x: 109, y: 20 })).toBe(true)
    expect(dragPast(down, { x: 94, y: 27 })).toBe(true)
    expect(dragPast(null, down)).toBe(false)
  })

  it('glows the edge the pointer went furthest past, the nearest one close inside, and none in the middle', () => {
    const room = { left: 14, top: 10, right: 1014, bottom: 810 }
    expect(nearEdge(room, { x: 500, y: 400 })).toBe('')
    expect(nearEdge(room, { x: 1040, y: 400 })).toBe('right')
    expect(nearEdge(room, { x: -5, y: 400 })).toBe('left')
    expect(nearEdge(room, { x: 500, y: -30 })).toBe('top')
    expect(nearEdge(room, { x: 1020, y: 860 })).toBe('bottom')
    expect(nearEdge(room, { x: 990, y: 400 })).toBe('right')
    expect(nearEdge(room, { x: 500, y: 790 }, 10)).toBe('')
    expect(nearEdge(null, { x: 1, y: 1 })).toBe('')
  })

  it('keeps the home its own tabs, its group and the tab in front among them', () => {
    expect(HOME).toBe('home')
    expect(atHome(list[4])).toBe(true)
    expect(atHome(list[1])).toBe(false)
    const view = homeView({ list, active: 2, panes: [1, 2], widths: [0.5, 0.5] })
    expect(view.list.map((t) => t.id)).toEqual([1, 5])
    expect(view.active).toBe(null)
    expect(view.panes).toEqual([])
    expect(homeView({ list, active: 1, panes: [] }).active).toBe(1)
    expect(homeView({ list: [...list, { id: 6, title: 'Six', window: 'home' }], active: 6, panes: [1, 6, 2] }).panes).toEqual([1, 6])
  })

  it('shows one dim chip per window of its own, a group that went out together as one', () => {
    const out = outChips(list)
    expect(out).toHaveLength(2)
    expect(out[0]).toEqual({ window: 'browser-out-1', ids: [2], title: 'Threads', list: 'Threads', unread: 3, private: false, danger: false })
    expect(out[1]).toMatchObject({ window: 'browser-out-2', ids: [3, 4], title: 'Video +1', unread: 1, private: true, danger: true })
    expect(outChips([list[0], list[4]])).toEqual([])
    expect(outChips(null)).toEqual([])
  })

  it('says where a drop would go, and nothing while it would stay', () => {
    expect(ghostLine('out')).toBe('Let go to open in a new window')
    expect(ghostLine('in')).toBe('Let go to bring it back')
    expect(ghostLine('stay')).toBe('')
  })
})
