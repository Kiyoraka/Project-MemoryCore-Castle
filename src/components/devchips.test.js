import { describe, expect, it } from 'vitest'
import { badgeChips, chipTitle, chipWords, chipsFor, devKey, devTrail, findDevTab, parseDevMarkers, peerOf, pidOf, withDevChips } from './devchips.js'

const dev = {
  tabs: [
    { slot: 0, folder: '/code/Sales Engineering', name: 'Sales Engineering', number: 1, pid: 4242 },
    { slot: 3, folder: '/code/montee', name: 'montee', number: 2, pid: 5151 },
    { slot: 1, folder: '/code/montee', name: 'montee', number: 1, pid: 6161 },
  ],
  projects: [
    { folder: '/code/Sales Engineering', name: 'Sales Engineering' },
    { folder: '/code/montee', name: 'montee' },
    { folder: '/code/tnex-solution', name: 'tnex-solution' },
  ],
}

describe('the DEV marker', () => {
  it('is taken out of the words and read with its state', () => {
    const got = parseDevMarkers('Montee finished #1276, the branch is green. [dev:montee · 1|done]')
    expect(got.text).toBe('Montee finished #1276, the branch is green.')
    expect(got.markers).toEqual([{ name: 'montee · 1', state: 'done' }])
  })

  it('reads every state, any case, and one with no state as FYI', () => {
    const got = parseDevMarkers('a [dev:shop|GO]\nb [dev: tnex-solution | fyi ]\nc [dev:montee]\nd [dev:sales|Done]')
    expect(got.text).toBe('a\nb\nc\nd')
    expect(got.markers.map((m) => `${m.name}:${m.state}`)).toEqual(['shop:go', 'tnex-solution:fyi', 'montee:fyi', 'sales:done'])
  })

  it('keeps one marker per tab (the first)', () => {
    expect(parseDevMarkers('x [dev:shop|go] y [dev:Shop|done]').markers).toEqual([{ name: 'shop', state: 'go' }])
  })

  it('leaves a malformed marker as written', () => {
    for (const text of ['[dev:]', '[dev:|go]', '[dev:shop|maybe]', '[dev:shop', 'dev:shop|go]', '[dev:a|go|done]', '[dev:sh[op|go]', `[dev:${'x'.repeat(81)}|go]`, '[dev:two\nlines|go]']) {
      const got = parseDevMarkers(text)
      expect(got.markers).toEqual([])
      expect(got.text).toBe(text)
    }
  })

  it('leaves markers in a fenced code block or inline code alone', () => {
    const text = 'Write it like this:\n```\nDone. [dev:shop|done]\n```\nor `[dev:shop|go]` inline. [dev:montee|fyi]'
    const got = parseDevMarkers(text)
    expect(got.markers).toEqual([{ name: 'montee', state: 'fyi' }])
    expect(got.text).toBe('Write it like this:\n```\nDone. [dev:shop|done]\n```\nor `[dev:shop|go]` inline.')
    const tilde = '~~~md\n[dev:shop|go]\n~~~'
    expect(parseDevMarkers(tilde)).toEqual({ text: tilde, markers: [] })
    // an unclosed fence keeps the rest as code
    expect(parseDevMarkers('```\n[dev:shop|go]').markers).toEqual([])
  })

  it('leaves words with no marker exactly as they were', () => {
    const text = 'Your 16:00 meeting is in 40 minutes.  \n\n'
    expect(parseDevMarkers(text)).toEqual({ text, markers: [] })
    expect(parseDevMarkers(null)).toEqual({ text: '', markers: [] })
  })
})

describe('which DEV tab a name or a session means', () => {
  it('compares names the way the tab strip writes them', () => {
    expect(devKey('montee · 2')).toBe(devKey('Montee-2'))
    expect(devKey('montee 2')).toBe('montee-2')
    expect(devKey('Sales Engineering')).toBe('sales-engineering')
  })

  it('reads the process id in a session address', () => {
    expect(pidOf('uds:/tmp/cc-socks/4242.sock')).toBe(4242)
    expect(pidOf('uds:/private/tmp/cc-socks-501/77.sock')).toBe(77)
    expect(pidOf('shop [k3]')).toBe(null)
  })

  it('finds a tab by its process id first', () => {
    expect(findDevTab({ name: 'whatever', pid: 5151 }, dev)).toEqual({ slot: 3, folder: '/code/montee', label: 'montee · 2', open: true })
  })

  it('finds a tab by its name, with or without its number', () => {
    expect(findDevTab({ name: 'sales-engineering' }, dev)).toEqual({ slot: 0, folder: '/code/Sales Engineering', label: 'Sales Engineering', open: true })
    expect(findDevTab({ name: 'montee · 2' }, dev).slot).toBe(3)
    expect(findDevTab({ name: 'Montee 2' }, dev).slot).toBe(3)
    // a project with several sessions, named without a number: its first session
    expect(findDevTab({ name: 'montee' }, dev)).toEqual({ slot: 1, folder: '/code/montee', label: 'montee · 1', open: true })
  })

  it('names a project with no session open as closed, and anything else as no tab', () => {
    expect(findDevTab({ name: 'tnex-solution' }, dev)).toEqual({ slot: null, folder: '/code/tnex-solution', label: 'tnex-solution', open: false })
    expect(findDevTab({ name: 'tnex-solution · 2' }, dev).open).toBe(false)
    expect(findDevTab({ name: 'montee · 7' }, dev)).toEqual({ slot: null, folder: '/code/montee', label: 'montee', open: false })
    expect(findDevTab({ name: 'nowhere' }, dev)).toBe(null)
    expect(findDevTab({ name: '' }, dev)).toBe(null)
    expect(findDevTab({ pid: 1 }, dev)).toBe(null)
    expect(findDevTab({ name: 'shop' }, {})).toBe(null)
  })

  it('reads who a peer entry names', () => {
    expect(peerOf({ role: 'peer', state: 'to', text: 'montee [k3]' })).toEqual({ name: 'montee', pid: null })
    expect(peerOf({ role: 'peer', state: 'to', text: 'uds:/tmp/cc-socks/4242.sock' })).toEqual({ name: '', pid: 4242 })
    expect(peerOf({ role: 'peer', state: 'from', text: 'shop\nuds:/tmp/cc-socks/9.sock' })).toEqual({ name: 'shop', pid: 9 })
    expect(peerOf({ role: 'peer', state: 'from', text: '\nuds:/tmp/cc-socks/9.sock' })).toEqual({ name: '', pid: 9 })
    expect(peerOf({ role: 'companion', text: 'x' })).toBe(null)
  })
})

describe('the trail', () => {
  const e = (id, role, text, state) => ({ id, role, text, state })

  it('puts a message heard on her next words', () => {
    const trail = devTrail([e('m1', 'member', 'how is montee?'), e('p1', 'peer', 'montee\nuds:/tmp/cc-socks/6161.sock', 'from'), e('c1', 'companion', 'Montee says all green.')])
    expect([...trail.entries()]).toEqual([['c1', [{ name: 'montee', pid: 6161 }]]])
  })

  it('puts a message sent on her words before it when none follow', () => {
    const trail = devTrail([e('m1', 'member', 'ask montee'), e('c1', 'companion', 'Asking montee now.'), e('w1', 'work', 'SendMessage'), e('p1', 'peer', 'montee [k3]', 'to'), e('m2', 'member', 'thanks')])
    expect(trail.get('c1')).toEqual([{ name: 'montee', pid: null }])
    expect(trail.size).toBe(1)
  })

  it('never carries a trail past the member or onto the talk before', () => {
    const trail = devTrail([e('c0', 'companion', 'Good morning.'), e('m1', 'member', 'hi'), e('p1', 'peer', 'montee [k3]', 'to'), e('m2', 'member', 'hello?')])
    expect(trail.size).toBe(0)
  })
})

describe('the chips on a message', () => {
  it('draws the marker with its state and adds the trail as FYI, one chip per tab', () => {
    const chips = chipsFor('c1', [{ name: 'montee · 1', state: 'done' }], [{ name: '', pid: 6161 }, { name: 'sales engineering', pid: null }], dev)
    expect(chips.map((c) => [c.label, c.state, c.from, c.slot])).toEqual([
      ['montee · 1', 'done', 'marker', 1],
      ['Sales Engineering', 'fyi', 'trail', 0],
    ])
    expect(chips[0].key).toBe('c1:1')
  })

  it('shows a marker naming no project as a chip that opens nothing; a trail to no tab shows none', () => {
    const chips = chipsFor('c1', [{ name: 'nowhere', state: 'go' }], [{ name: 'some other session', pid: 99 }], dev)
    expect(chips).toHaveLength(1)
    expect(chips[0]).toMatchObject({ label: 'nowhere', open: false, unknown: true, slot: null })
    expect(chipWords(chips[0])).toBe('no DEV tab by that name')
  })

  it('words a chip by its state, or as closed', () => {
    expect(chipWords({ open: true, state: 'go' })).toBe('needs your go')
    expect(chipWords({ open: true, state: 'done' })).toBe('finished')
    expect(chipWords({ open: true, state: 'fyi' })).toBe('FYI')
    expect(chipWords({ open: false, label: 'montee' })).toBe('closed')
    expect(chipTitle({ open: true, label: 'montee · 2' })).toBe('Open montee · 2 in DEV')
  })

  it('gives HOME its chips and hides the markers everywhere else', () => {
    const entries = [
      { id: 'm1', role: 'member', text: 'status?' },
      { id: 'c1', role: 'companion', text: 'Sales waits for you. [dev:Sales Engineering|go]' },
      { id: 'c2', role: 'companion', text: 'Your meeting is at 16:00.' },
    ]
    const items = entries.map((entry) => ({ kind: 'message', id: entry.id, entry }))
    const home = withDevChips(items, entries, dev)
    expect(home[1].shown).toBe('Sales waits for you.')
    expect(home[1].dev.map((c) => c.state)).toEqual(['go'])
    expect(home[0]).toBe(items[0])
    expect(home[2]).toBe(items[2])
    const other = withDevChips(items, entries, dev, { chips: false })
    expect(other[1].shown).toBe('Sales waits for you.')
    expect(other[1].dev).toEqual([])
  })
})

describe('the DEV button badge', () => {
  const item = (id, at, dev) => ({ kind: 'message', id, entry: { id, role: 'companion', at }, dev })
  it('counts needs-your-go and finished chips on open tabs said since Castle started, not seen yet', () => {
    const items = [
      item('old', '2026-11-26T08:00:00Z', [{ key: 'old:1', slot: 1, open: true, state: 'done' }]),
      item('a', '2026-11-26T10:00:00Z', [
        { key: 'a:1', slot: 1, open: true, state: 'done' },
        { key: 'a:0', slot: 0, open: true, state: 'fyi' },
      ]),
      item('b', '2026-11-26T10:05:00Z', [
        { key: 'b:0', slot: 0, open: true, state: 'go' },
        { key: 'b:x', slot: null, open: false, state: 'go' },
      ]),
    ]
    const since = Date.parse('2026-11-26T09:00:00Z')
    expect(badgeChips(items, since)).toEqual([{ key: 'a:1', slot: 1 }, { key: 'b:0', slot: 0 }])
    expect(badgeChips(items, since, new Set(['a:1']))).toEqual([{ key: 'b:0', slot: 0 }])
  })
})
