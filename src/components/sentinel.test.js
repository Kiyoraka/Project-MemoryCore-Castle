import { describe, expect, it } from 'vitest'
import { MAC_LINE, STALE_MINUTES, findingButtons, moveLabel, networkLine, patrolAgo, shieldLook, watchAgo } from './sentinel.js'

describe('the card on HOME says the watch is working', () => {
  it('tells how long ago the last patrol ran', () => {
    expect(patrolAgo(null)).toBe('No patrol yet.')
    expect(patrolAgo({ patrol: null })).toBe('No patrol yet.')
    expect(patrolAgo({ patrol: { minutesAgo: 0 } })).toBe('Last patrol just now.')
    expect(patrolAgo({ patrol: { minutesAgo: 12 } })).toBe('Last patrol 12 min ago.')
    expect(patrolAgo({ patrol: { minutesAgo: 190 } })).toBe('Last patrol 3 h ago.')
  })

  it('tells how long ago the watch ran, and flags it once it has missed its ticks', () => {
    expect(watchAgo({ watchMinutes: null })).toEqual({ line: 'The watch has not run yet.', late: false })
    expect(watchAgo(null)).toEqual({ line: 'The watch has not run yet.', late: false })
    expect(watchAgo({ watchMinutes: 0 })).toEqual({ line: 'The watch ran just now.', late: false })
    expect(watchAgo({ watchMinutes: 3 })).toEqual({ line: 'The watch ran 3 min ago.', late: false })
    expect(watchAgo({ watchMinutes: 15 }).late).toBe(false)
    expect(watchAgo({ watchMinutes: 16 })).toEqual({ line: 'The watch ran 16 min ago.', late: true })
    expect(watchAgo({ watchMinutes: 300 })).toEqual({ line: 'The watch ran 5 h ago.', late: true })
  })
})

const ready = (extra = {}) => ({
  supported: true,
  on: true,
  installed: true,
  current: true,
  setupLine: 'The Sentinel is set up.',
  patrol: { status: 'quiet', line: 'All quiet.', minutesAgo: 10 },
  patrolling: false,
  unread: 0,
  open: { alarm: 0, look: 0, note: 3 },
  problem: null,
  ...extra,
})

describe('the shield', () => {
  it('is quiet with the companion sentence when all is well', () => {
    expect(shieldLook(ready())).toEqual({ look: 'quiet', line: 'All quiet.', stale: false })
  })

  it('glows for an unread alarm, an open alarm or an alarm verdict', () => {
    expect(shieldLook(ready({ unread: 1 })).look).toBe('alarm')
    expect(shieldLook(ready({ open: { alarm: 1, look: 0, note: 0 } })).look).toBe('alarm')
    expect(shieldLook(ready({ patrol: { status: 'alarm', line: 'Look now.', minutesAgo: 1 } })).look).toBe('alarm')
  })

  it('asks for a look for a look verdict, an open look, or a hand that needs a refresh', () => {
    expect(shieldLook(ready({ patrol: { status: 'look', line: 'When you can.', minutesAgo: 1 } })).look).toBe('look')
    expect(shieldLook(ready({ open: { alarm: 0, look: 2, note: 0 } })).look).toBe('look')
    const old = shieldLook(ready({ current: false, setupLine: 'The Sentinel needs a refresh after an update: run the line again.' }))
    expect(old.look).toBe('look')
    expect(old.line).toMatch(/refresh/)
  })

  it('fades when the last patrol is old or there is none', () => {
    expect(shieldLook(ready({ patrol: { status: 'quiet', line: 'x', minutesAgo: STALE_MINUTES + 1 } })).stale).toBe(true)
    const none = shieldLook(ready({ patrol: null }))
    expect(none.stale).toBe(true)
    expect(none.line).toMatch(/Patrol now/)
  })

  it('is dim and says so when off or not set up, and tells a Mac it keeps watch on Windows for now', () => {
    expect(shieldLook(ready({ on: false })).look).toBe('unset')
    expect(shieldLook(ready({ installed: false })).line).toMatch(/set-up line/)
    expect(shieldLook(ready({ supported: false }))).toEqual({ look: 'away', line: MAC_LINE, stale: false })
    expect(shieldLook(null).look).toBe('unset')
  })

  it('says it is on patrol while one runs, and shows a problem reading the book', () => {
    expect(shieldLook(ready({ patrolling: true })).line).toMatch(/On patrol/)
    expect(shieldLook(ready({ problem: 'The Sentinel keeps its book in your memory database.' })).line).toMatch(/memory database/)
  })
})

describe('the buttons', () => {
  it('an open finding with a move offers Fix and This is normal, without one only This is normal', () => {
    expect(findingButtons({ state: 'open', fixMove: 'block-port' })).toEqual(['fix', 'accept'])
    expect(findingButtons({ state: 'open', fixMove: null })).toEqual(['accept'])
  })

  it('a fixed finding offers Undo only when its fix left a recipe; settled and accepted offer nothing', () => {
    expect(findingButtons({ state: 'fixed', undoable: true })).toEqual(['undo'])
    expect(findingButtons({ state: 'fixed', undoable: false })).toEqual([])
    expect(findingButtons({ state: 'settled' })).toEqual([])
    expect(findingButtons({ state: 'accepted' })).toEqual([])
    expect(findingButtons(null)).toEqual([])
  })

  it('names each move in the member words', () => {
    expect(moveLabel('block-port')).toBe('Block this port')
    expect(moveLabel('disable-startup')).toBe('Stop it starting with Windows')
    expect(moveLabel('something-new')).toBe('Fix')
  })
})

describe('the Aegis Network on the sheet', () => {
  it('says whether the member joined and what was blocked today', () => {
    expect(networkLine({ network: false })).toBe('The Aegis Network: not joined (Settings > Sentinel).')
    expect(networkLine({ network: true, networkBlocked: 0 })).toBe('The Aegis Network: joined. Nothing blocked today.')
    expect(networkLine({ network: true, networkBlocked: 1 })).toBe('The Aegis Network: joined, 1 address blocked today.')
    expect(networkLine({ network: true, networkBlocked: 3 })).toBe('The Aegis Network: joined, 3 addresses blocked today.')
    expect(networkLine(null)).toMatch(/not joined/)
  })

  it('offers Undo on a block the network made, as on any fix', () => {
    expect(findingButtons({ kind: 'network-address', state: 'fixed', undoable: true })).toEqual(['undo'])
  })
})
