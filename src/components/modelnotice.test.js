import { describe, expect, it } from 'vitest'
import { fullLineFor, noticeKey, noticeWords } from './modelnotice.js'

const now = Date.UTC(2026, 10, 25, 1, 0)
const notice = { share: 'Fable', model: 'opus', untilMs: Date.UTC(2026, 10, 27, 5, 0) }

describe('the share fallback line', () => {
  it('names the share, the reset, the companion and the model', () => {
    const line = noticeWords(notice, 'Alice', now, 'en-GB')
    expect(line.startsWith('The Fable weekly share is spent until ')).toBe(true)
    expect(line.endsWith(': Alice is thinking with Opus until then.')).toBe(true)
    expect(line).toMatch(/27 Nov/)
  })

  it('says nothing without a notice or once its reset has passed', () => {
    expect(noticeWords(null, 'Alice', now)).toBe('')
    expect(noticeWords({ ...notice, untilMs: now - 1 }, 'Alice', now)).toBe('')
    expect(noticeWords({ ...notice, model: '' }, 'Alice', now)).toBe('')
  })

  it('keeps an unknown model as it is, and keys a notice by its share and reset', () => {
    expect(noticeWords({ ...notice, model: 'haiku' }, 'Rin', now)).toMatch(/thinking with haiku until then/)
    expect(noticeKey(notice)).toBe(`Fable@${notice.untilMs}`)
    expect(noticeKey(null)).toBe('')
  })
})

describe('the line of a session that keeps filling up', () => {
  it('reaches only its own panel', () => {
    const tab = { slot: 2, line: 'This session keeps filling up.' }
    expect(fullLineFor(tab, 2)).toBe('This session keeps filling up.')
    expect(fullLineFor(tab, 3)).toBeNull()
    expect(fullLineFor(tab, null)).toBeNull()
    expect(fullLineFor({ slot: null, line: 'home' }, null)).toBe('home')
    expect(fullLineFor({ slot: null, line: 'home' }, 0)).toBeNull()
  })

  it('an empty line clears it and nonsense is ignored', () => {
    expect(fullLineFor({ slot: 1, line: '' }, 1)).toBe('')
    expect(fullLineFor(null, 1)).toBeNull()
    expect(fullLineFor({ slot: 1 }, 1)).toBeNull()
  })
})
