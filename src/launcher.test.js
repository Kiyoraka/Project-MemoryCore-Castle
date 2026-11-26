import { describe, expect, it } from 'vitest'
import { windowRoom as windowRoomOf } from './launcher.js'
import { DEFAULT_PINS, FIXED, GROUPS, HINTS, LAUNCHER_TAB, cleanPins, pinRows, groupRooms, headerTabs, initialOf, setPinned, tipDelay, TIP_FIRST_MS, windowRoom } from './launcher.js'

const room = (id, label = id.toUpperCase()) => ({ id, label })
const all = ['home', 'theme', 'apps', 'myprofile', 'diary', 'projects', 'library', 'appearance', 'studio', 'story', 'world', 'chat', 'dev', 'market', 'messaging', 'automations', 'calendar', 'finance', 'browser', 'artifacts', 'worldsmith', 'scriptorium', 'photo'].map((id) => room(id))
const ids = (groups) => groups.flatMap((g) => g.tabs.map((t) => t.id))

describe('the launcher groups', () => {
  it('puts every known room in exactly one group', () => {
    const groups = groupRooms(all)
    expect(ids(groups).sort()).toEqual(all.map((t) => t.id).sort())
    expect(new Set(ids(groups)).size).toBe(all.length)
    expect(groups.map((g) => g.label)).toEqual(['Companion', 'Work', 'Create', 'Connect', 'System'])
  })
  it('lists a room in no more than one group of the list itself', () => {
    const listed = GROUPS.flatMap((g) => g.rooms)
    expect(new Set(listed).size).toBe(listed.length)
  })
  it('puts a room it does not know into a last group, so a new room is never lost', () => {
    const groups = groupRooms([...all, room('newroom', 'NEW ROOM')])
    expect(groups.at(-1).label).toBe('More')
    expect(groups.at(-1).tabs.map((t) => t.id)).toEqual(['newroom'])
    expect(ids(groups).length).toBe(all.length + 1)
  })
  it('leaves out a group with no visible room, and a room that is switched off is simply not in the list', () => {
    const fewer = all.filter((t) => !['studio', 'photo', 'story', 'world', 'library', 'worldsmith'].includes(t.id))
    const groups = groupRooms(fewer)
    expect(groups.map((g) => g.id)).toEqual(['companion', 'work', 'connect', 'system'])
    expect(ids(groups)).not.toContain('studio')
  })
  it('keeps the listed order inside a group, whatever order the tabs come in', () => {
    const shuffled = [...all].reverse()
    expect(groupRooms(shuffled)[0].tabs.map((t) => t.id)).toEqual(['home', 'chat', 'myprofile', 'diary', 'appearance', 'finance'])
  })
  it('copes with nothing', () => {
    expect(groupRooms([])).toEqual([])
    expect(groupRooms(undefined)).toEqual([])
  })
  it('has a hint for every room Castle has', () => {
    for (const t of all) expect(HINTS[t.id], t.id).toBeTruthy()
  })
})

describe('window rooms', () => {
  it('BROWSER opens the browser window on a new tab, ARTIFACTS on the artifacts page', () => {
    expect(windowRoom('browser')).toEqual({ url: null })
    expect(windowRoom('artifacts')).toEqual({ url: 'https://claude.ai/code/artifacts' })
  })
  it('every other room is a room', () => {
    for (const id of ['home', 'dev', 'calendar', 'launcher', '', undefined]) expect(windowRoom(id)).toBe(null)
  })
  it('both sit in Work', () => {
    expect(GROUPS.find((g) => g.id === 'work').rooms).toEqual(expect.arrayContaining(['browser', 'artifacts']))
  })
  it('SCRIPTORIUM sits in Work, right after DEV, with its hint', () => {
    const work = GROUPS.find((g) => g.id === 'work').rooms
    expect(work.indexOf('scriptorium')).toBe(work.indexOf('dev') + 1)
    expect(HINTS.scriptorium).toBe('Your own documents, sheets, slides and PDFs')
    expect(windowRoom('scriptorium')).toBe(null)
  })
  it('PHOTO sits in Create, right after STUDIO, with its hint', () => {
    const create = GROUPS.find((g) => g.id === 'create').rooms
    expect(create.indexOf('photo')).toBe(create.indexOf('studio') + 1)
    expect(HINTS.photo).toBe('A basic photo editor with your companion')
    expect(windowRoom('photo')).toBe(null)
  })
})

describe('pins', () => {
  const known = all.map((t) => t.id)
  it('the first pins are CHAT, PROJECTS and DEV', () => {
    expect(DEFAULT_PINS).toEqual(['chat', 'projects', 'dev'])
  })
  it('keeps real rooms once each in order, and never HOME or the launcher', () => {
    expect(cleanPins([' dev ', 'home', 'launcher', 'dev', '', 'nothing', 'calendar', 'chat'], known)).toEqual(['dev', 'calendar', 'chat'])
    expect(cleanPins(undefined, known)).toEqual([])
    expect(cleanPins(['x'.repeat(5)], known)).toEqual([])
  })
  it('pins and unpins a room, putting a new pin last', () => {
    expect(setPinned(['chat'], 'dev', true, known)).toEqual(['chat', 'dev'])
    expect(setPinned(['chat', 'dev'], 'chat', false, known)).toEqual(['dev'])
    expect(setPinned(['chat', 'dev'], 'chat', true, known)).toEqual(['dev', 'chat'])
  })
  it('cannot pin or unpin HOME or the launcher', () => {
    for (const id of FIXED) {
      expect(setPinned(['chat'], id, true, known)).toEqual(['chat'])
      expect(setPinned(['chat'], id, false, known)).toEqual(['chat'])
    }
  })
  it('caps the number of pins', () => {
    const many = Array.from({ length: 40 }, (_, i) => `r${i}`)
    expect(cleanPins(many).length).toBe(16)
  })
  it('Settings lists every APPS tile but HOME and APPS, in the tiles\' order, window tiles included', () => {
    const labels = { home: 'HOME', launcher: 'APPS', chat: 'CHAT', projects: 'PROJECTS', browser: 'BROWSER', artifacts: 'ARTIFACTS', theme: 'THEME' }
    expect(pinRows(labels).map((r) => r.id)).toEqual(['chat', 'projects', 'browser', 'artifacts', 'theme'])
    expect(pinRows({})).toEqual([])
  })
  it('a pin switched in Settings keeps a window tile pinned on the tiles', () => {
    const known = pinRows({ chat: 'CHAT', browser: 'BROWSER', artifacts: 'ARTIFACTS' }).map((r) => r.id)
    expect(setPinned(['browser', 'artifacts'], 'chat', true, known)).toEqual(['browser', 'artifacts', 'chat'])
  })
})

describe('the top bar', () => {
  const visible = all.filter((t) => t.id !== 'calendar')
  it('holds HOME, APPS and the pinned rooms in the member tab order', () => {
    const bar = headerTabs(visible, ['dev', 'chat'], 'home')
    expect(bar.map((t) => t.id)).toEqual(['home', 'launcher', 'chat', 'dev'])
    expect(bar[1]).toEqual(LAUNCHER_TAB)
    expect(bar[1].label).toBe('APPS')
  })
  it('keeps HOME and APPS fixed so they cannot be dragged', () => {
    const bar = headerTabs(visible, [], 'home')
    expect(bar.map((t) => [t.id, Boolean(t.fixed)])).toEqual([['home', true], ['launcher', true]])
  })
  it('shows the open room as a tab even when it is not pinned', () => {
    const bar = headerTabs(visible, ['chat'], 'studio')
    expect(bar.map((t) => t.id)).toEqual(['home', 'launcher', 'studio', 'chat'])
  })
  it('does not double a pinned room that is open, nor list a room that is switched off', () => {
    expect(headerTabs(visible, ['chat'], 'chat').map((t) => t.id)).toEqual(['home', 'launcher', 'chat'])
    expect(headerTabs(visible, ['calendar'], 'home').map((t) => t.id)).toEqual(['home', 'launcher'])
  })
  it('shows only HOME and APPS while the launcher itself is open and nothing is pinned', () => {
    expect(headerTabs(visible, [], 'launcher').map((t) => t.id)).toEqual(['home', 'launcher'])
  })
  it('is short: HOME, APPS and the default pins are five tabs, not seventeen', () => {
    expect(headerTabs(all, DEFAULT_PINS, 'home').length).toBe(5)
  })
})

describe('tile letters', () => {
  it('uses the first letter of the name', () => {
    expect(initialOf('CHAT')).toBe('C')
    expect(initialOf('  my profile')).toBe('M')
    expect(initialOf('')).toBe('?')
    expect(initialOf(null)).toBe('?')
  })
})

import { badgesFor, filterGroups, filterTabs, firstMatch, moveFocus } from './launcher.js'

describe('search', () => {
  const rooms = all
  it('matches the name, whatever the case', () => {
    expect(filterTabs(rooms, 'CHAT')[0].id).toBe('chat')
    expect(filterTabs(rooms, 'dev')[0].id).toBe('dev')
    expect(filterTabs(rooms, 'DeV').map((t) => t.id)).toEqual(filterTabs(rooms, 'dev').map((t) => t.id))
    expect(filterTabs(rooms, 'my pro').map((t) => t.id)).toEqual(['myprofile'])
  })
  it('puts a name match before a hint match, so Enter opens the room you typed', () => {
    const ids = filterTabs(rooms, 'chat').map((t) => t.id)
    expect(ids[0]).toBe('chat')
    expect(ids).toContain('home')
    expect(ids.indexOf('chat')).toBeLessThan(ids.indexOf('home'))
  })
  it('puts a name that starts with the word before one that only contains it', () => {
    const ids = filterTabs([room('apps', 'DESKTOP'), room('theme', 'THEME'), room('dev', 'DEV')], 'de').map((t) => t.id)
    expect(ids.slice(0, 2)).toEqual(['apps', 'dev'])
  })
  it('matches the hint as well as the name', () => {
    expect(filterTabs(rooms, 'terminals').map((t) => t.id)).toEqual(['dev'])
    expect(filterTabs(rooms, 'start-menu').map((t) => t.id)).toEqual(['apps'])
  })
  it('wants every word to match', () => {
    expect(filterTabs(rooms, 'jobs schedule').map((t) => t.id)).toEqual(['automations'])
    expect(filterTabs(rooms, 'jobs banana')).toEqual([])
  })
  it('returns every room for an empty search', () => {
    expect(filterTabs(rooms, '')).toEqual(rooms)
    expect(filterTabs(rooms, '   ')).toEqual(rooms)
    expect(filterTabs(rooms, null)).toEqual(rooms)
  })
  it('says nothing matches with an empty list', () => {
    expect(filterTabs(rooms, 'zzzz')).toEqual([])
    expect(filterGroups(groupRooms(rooms), 'zzzz')).toEqual([])
  })
  it('shows one list of matches, best first, and Enter opens the first one shown', () => {
    const groups = filterGroups(groupRooms(rooms), 'calendar')
    expect(groups.map((g) => g.id)).toEqual(['matches'])
    expect(firstMatch(groups).id).toBe('calendar')
    expect(firstMatch(filterGroups(groupRooms(rooms), 'chat')).id).toBe('chat')
    expect(firstMatch([])).toBe(null)
    expect(firstMatch(groupRooms(rooms)).id).toBe('home')
  })
  it('does not change the groups when nothing is typed', () => {
    const groups = groupRooms(rooms)
    expect(filterGroups(groups, '')).toBe(groups)
  })
})

describe('arrow keys', () => {
  // two groups on a three-column grid: 5 tiles (a short last row of 2) and 4 tiles (a short last row of 1)
  const sizes = [5, 4]
  const go = (g, i, key) => moveFocus(sizes, { g, i }, key, 3)
  it('Left and Right walk through every tile and wrap around', () => {
    expect(go(0, 0, 'ArrowRight')).toEqual({ g: 0, i: 1 })
    expect(go(0, 4, 'ArrowRight')).toEqual({ g: 1, i: 0 })
    expect(go(1, 3, 'ArrowRight')).toEqual({ g: 0, i: 0 })
    expect(go(0, 0, 'ArrowLeft')).toEqual({ g: 1, i: 3 })
    expect(go(1, 0, 'ArrowLeft')).toEqual({ g: 0, i: 4 })
  })
  it('Down keeps the column inside a group', () => {
    expect(go(0, 0, 'ArrowDown')).toEqual({ g: 0, i: 3 })
    expect(go(0, 1, 'ArrowDown')).toEqual({ g: 0, i: 4 })
  })
  it('Down from a column the short last row lacks stops at that row end', () => {
    expect(go(0, 2, 'ArrowDown')).toEqual({ g: 0, i: 4 })
  })
  it('Down from the last row goes to the same column of the next group, and wraps to the first', () => {
    expect(go(0, 3, 'ArrowDown')).toEqual({ g: 1, i: 0 })
    expect(go(0, 4, 'ArrowDown')).toEqual({ g: 1, i: 1 })
    expect(go(1, 3, 'ArrowDown')).toEqual({ g: 0, i: 0 })
    expect(go(1, 2, 'ArrowDown')).toEqual({ g: 1, i: 3 }) // not the last row yet: stops at the end of the short one
  })
  it('Up keeps the column, then goes to the last row of the group before, and wraps to the last group', () => {
    expect(go(0, 4, 'ArrowUp')).toEqual({ g: 0, i: 1 })
    expect(go(1, 1, 'ArrowUp')).toEqual({ g: 0, i: 4 }) // column 1 of the short last row
    expect(go(1, 2, 'ArrowUp')).toEqual({ g: 0, i: 4 }) // column 2 is not there: the row end
    expect(go(0, 1, 'ArrowUp')).toEqual({ g: 1, i: 3 }) // wraps to the last group, whose last row has only column 0
    expect(go(0, 0, 'ArrowUp')).toEqual({ g: 1, i: 3 })
  })
  it('Home and End go to the first and last tile', () => {
    expect(go(1, 2, 'Home')).toEqual({ g: 0, i: 0 })
    expect(go(0, 1, 'End')).toEqual({ g: 1, i: 3 })
  })
  it('skips an empty group and copes with nothing or one tile', () => {
    expect(moveFocus([2, 0, 2], { g: 0, i: 1 }, 'ArrowRight', 3)).toEqual({ g: 2, i: 0 })
    expect(moveFocus([2, 0, 2], { g: 0, i: 0 }, 'ArrowDown', 3)).toEqual({ g: 2, i: 0 })
    expect(moveFocus([], { g: 0, i: 0 }, 'ArrowRight', 3)).toBe(null)
    expect(moveFocus([1], { g: 0, i: 0 }, 'ArrowDown', 3)).toEqual({ g: 0, i: 0 })
    expect(moveFocus([1], { g: 0, i: 0 }, 'ArrowLeft', 3)).toEqual({ g: 0, i: 0 })
  })
  it('starts at the first tile when the focus is not on a tile yet', () => {
    expect(moveFocus(sizes, null, 'ArrowRight', 3)).toEqual({ g: 0, i: 1 })
    expect(moveFocus(sizes, { g: 9, i: 9 }, 'ArrowDown', 3)).toEqual({ g: 0, i: 3 })
  })
  it('ignores a key it does not know', () => {
    expect(go(0, 1, 'a')).toEqual({ g: 0, i: 1 })
  })
})

describe('badges', () => {
  const now = new Date(2026, 10, 22, 12, 0)
  const bad = { name: 'Backup', status: 'failed', at: '2026-11-21 09:00:00' }
  it('is empty when nothing is waiting', () => {
    expect(badgesFor({ automations: { notices: [], paused: false }, calendar: { next: [], notices: [], proposed: 0 } }, now)).toEqual({})
    expect(badgesFor({}, now)).toEqual({})
  })
  it("shows the browser tabs' news on the BROWSER tile, with each site", () => {
    expect(badgesFor({ browser: { count: 15, title: 'WhatsApp 3 · Gmail 12' } }, now).browser).toEqual({ count: 15, tone: 'info', text: 'New in your tabs: WhatsApp 3 · Gmail 12' })
    expect(badgesFor({ browser: { count: 0, title: '' } }, now).browser).toBeUndefined()
    expect(badgesFor({ marks: ['browser'], browser: { count: 2, title: '' } }, now).browser.count).toBe(2)
  })
  it('counts recent failed automation runs, and ignores old or successful ones', () => {
    const old = { ...bad, at: '2026-11-01 09:00:00' }
    const ok = { name: 'Brief', status: 'ok', at: '2026-11-22 08:00:00' }
    const b = badgesFor({ automations: { notices: [bad, { ...bad, status: 'timed_out' }, old, ok], paused: false } }, now)
    expect(b.automations).toMatchObject({ count: 2, tone: 'bad' })
    expect(b.automations.text).toContain('2 recent jobs failed')
    expect(badgesFor({ automations: { notices: [old, ok], paused: false } }, now).automations).toBeUndefined()
  })
  it('says paused when nothing failed, and failures win over paused', () => {
    expect(badgesFor({ automations: { notices: [], paused: true } }, now).automations).toEqual({ count: 0, tone: 'warn', text: 'Automations are paused' })
    expect(badgesFor({ automations: { notices: [bad], paused: true } }, now).automations.tone).toBe('bad')
  })
  it('counts the chats waiting for approval on MESSAGING', () => {
    expect(badgesFor({ pending: 2 }, now).messaging).toMatchObject({ count: 2, tone: 'warn' })
    expect(badgesFor({ pending: 1 }, now).messaging.text).toBe('1 chat is waiting for your approval')
    expect(badgesFor({ pending: 0 }, now).messaging).toBeUndefined()
  })
  it('marks a room with something new as a plain dot', () => {
    const b = badgesFor({ marks: ['chat', 'story'] }, now)
    expect(b.chat).toEqual({ count: 0, tone: 'info', text: 'New in the room' })
    expect(b.story.text).toBe('The story has new lines')
  })
  it('counts what needs a look in CALENDAR: events today, reminders raised, suggestions waiting', () => {
    const calendar = { next: [{ date: '2026-11-22', title: 'A' }, { date: '2026-11-22', title: 'B' }, { date: '2026-11-25', title: 'C' }], notices: [{ id: 1 }], proposed: 1 }
    const b = badgesFor({ calendar }, now).calendar
    expect(b.count).toBe(4)
    expect(b.text).toBe('2 events today, 1 reminder, 1 suggestion waiting')
    expect(badgesFor({ calendar: { next: [{ date: '2026-11-25' }], notices: [], proposed: 0 } }, now).calendar).toBeUndefined()
  })
  it('reads only what it is given and never throws on odd data', () => {
    expect(() => badgesFor({ automations: { notices: [{ status: 'failed', at: 'nonsense' }, null].filter(Boolean) }, calendar: { next: [{}] } }, now)).not.toThrow()
    expect(badgesFor({ automations: { notices: [{ status: 'failed', at: 'nonsense' }] } }, now).automations).toBeUndefined()
  })
})

import { opensLauncher } from './launcher.js'

describe('Ctrl+K', () => {
  const press = (extra = {}) => ({ key: 'k', ctrlKey: true, metaKey: false, altKey: false, shiftKey: false, repeat: false, ...extra })
  it('opens APPS from any ordinary room, with Ctrl or Cmd', () => {
    for (const room of ['home', 'chat', 'projects', 'launcher', 'calendar']) expect(opensLauncher(press(), { room }), room).toBe(true)
    expect(opensLauncher(press({ ctrlKey: false, metaKey: true }), { room: 'home' })).toBe(true)
    expect(opensLauncher(press({ key: 'K' }), { room: 'home' })).toBe(true)
  })
  it('leaves the terminal in DEV alone', () => {
    expect(opensLauncher(press(), { room: 'dev' })).toBe(false)
  })
  it('leaves a sheet over Castle alone', () => {
    expect(opensLauncher(press(), { room: 'home', sheetOpen: true })).toBe(false)
  })
  it('wants exactly Ctrl or Cmd with K', () => {
    expect(opensLauncher(press({ ctrlKey: false }), { room: 'home' })).toBe(false)
    expect(opensLauncher(press({ key: 'j' }), { room: 'home' })).toBe(false)
    expect(opensLauncher(press({ shiftKey: true }), { room: 'home' })).toBe(false)
    expect(opensLauncher(press({ altKey: true }), { room: 'home' })).toBe(false)
    expect(opensLauncher(press({ repeat: true }), { room: 'home' })).toBe(false)
    expect(opensLauncher(null)).toBe(false)
  })
})

describe('tile tooltips', () => {
  it('waits for the first two tips and shows them at once from the third', () => {
    expect(tipDelay(0)).toBe(TIP_FIRST_MS)
    expect(tipDelay(1)).toBe(TIP_FIRST_MS)
    expect(tipDelay(2)).toBe(0)
    expect(tipDelay(5)).toBe(0)
  })
})

describe('the browser inside Castle', () => {
  it('makes BROWSER a room of its own, while ARTIFACTS still opens the browser', () => {
    expect(windowRoomOf('browser', 'castle')).toBe(null)
    expect(windowRoomOf('artifacts', 'castle')).toEqual({ url: 'https://claude.ai/code/artifacts' })
    expect(windowRoomOf('browser', 'window')).toEqual({ url: null })
    expect(windowRoomOf('browser')).toEqual({ url: null }, 'its own window to begin with')
  })
})
