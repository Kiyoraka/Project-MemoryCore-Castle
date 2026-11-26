import { describe, expect, it } from 'vitest'
import { ASK_AGAIN_AT, ASK_AT, cardWords, nextCard, notNow, restingFresh, showsCard } from './freshstart.js'

const at = (percent, extra = {}) => ({ percent, busy: false, running: true, ...extra })

describe('the home chat asks to start fresh', () => {
  it('asks at 85%, only while awake and between turns', () => {
    expect(nextCard('watching', at(ASK_AT - 1))).toBe('watching')
    expect(nextCard('watching', at(ASK_AT))).toBe('asking')
    expect(nextCard('watching', at(90, { busy: true }))).toBe('watching')
    expect(nextCard('watching', at(90, { running: false }))).toBe('watching')
    expect(nextCard('watching', { percent: null, busy: false, running: true })).toBe('watching')
    expect(showsCard('asking')).toBe(true)
  })

  it('Not now asks once more at 95%, then stays quiet until a new session', () => {
    let state = notNow('asking')
    expect(state).toBe('snoozed')
    expect(showsCard(state)).toBe(false)
    expect(nextCard(state, at(ASK_AGAIN_AT - 1))).toBe('snoozed')
    state = nextCard(state, at(ASK_AGAIN_AT))
    expect(state).toBe('asking-again')
    state = notNow(state)
    expect(state).toBe('quiet')
    expect(nextCard(state, at(99))).toBe('quiet')
    expect(nextCard(state, at(12))).toBe('watching')
  })

  it('a fresh start in progress is never interrupted by the meter', () => {
    expect(nextCard('running', at(5))).toBe('running')
  })

  it('speaks as the companion and promises only what is kept', () => {
    expect(cardWords(86)).toBe('My memory of this talk is getting full (86%). Shall I start fresh? Nothing is lost: Chat keeps every word, I keep my memory, and I carry our latest talk with me.')
    expect(cardWords(undefined)).toContain('getting full. Shall I')
    expect(cardWords(86).toLowerCase()).not.toContain('alice')
  })

  it('after a launch that never got going, the resting Start fresh tries the fresh start again', () => {
    expect(restingFresh({ offersFresh: true, freshRetry: true })).toBe('retry')
    // even for a member whose every start is fresh (no one-off button otherwise)
    expect(restingFresh({ offersFresh: false, freshRetry: true })).toBe('retry')
    expect(restingFresh({ offersFresh: true, freshRetry: false })).toBe('new')
    expect(restingFresh({ offersFresh: false, freshRetry: false })).toBe(null)
  })
})
