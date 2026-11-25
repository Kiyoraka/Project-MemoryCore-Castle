import { describe, expect, it } from 'vitest'
import { clearNotSent, localSend, markNotSent, mergeEntries, resetEntries, stillSending } from './ticks.js'

const states = (list) => list.map((m) => [m.role, m.text, m.state ?? null])

describe("the member's message ticks", () => {
  it('sent while the companion works: sending, then queued, then read when it is taken in mid-turn', () => {
    let list = [localSend('add upload and download')]
    expect(states(list)).toEqual([['member', 'add upload and download', 'sending']])
    list = mergeEntries(list, [{ id: 'q1', role: 'member', text: 'add upload and download', state: 'queued' }])
    expect(states(list)).toEqual([['member', 'add upload and download', 'queued']])
    list = mergeEntries(list, [{ id: 'r1', role: 'receipt', text: 'add upload and download', state: 'absorbed_mid_turn' }])
    expect(states(list)).toEqual([['member', 'add upload and download', 'read']])
  })

  it('queued, then read as the next turn: shown once', () => {
    let list = mergeEntries([], [{ id: 'q1', role: 'member', text: 'one  more\nthing', state: 'queued' }])
    list = mergeEntries(list, [
      { id: 'r1', role: 'receipt', text: 'one more thing', state: 'dequeued' },
      { id: 'u1', role: 'member', text: 'one more thing' },
      { id: 'a1', role: 'companion', text: 'yes' },
    ])
    expect(states(list)).toEqual([['member', 'one  more\nthing', 'read'], ['companion', 'yes', null]])
  })

  it('sent while the companion is idle: sending, then read', () => {
    let list = [{ id: 'a0', role: 'companion', text: 'hi' }, localSend('hello')]
    list = mergeEntries(list, [{ id: 'u1', role: 'member', text: 'hello' }])
    expect(states(list)).toEqual([['companion', 'hi', null], ['member', 'hello', 'read']])
  })

  it('a queued message taken back leaves Chat', () => {
    let list = mergeEntries([], [{ id: 'q1', role: 'member', text: 'oops', state: 'queued' }])
    list = mergeEntries(list, [{ id: 'r1', role: 'receipt', text: 'oops', state: 'popped_for_edit' }])
    expect(list).toEqual([])
  })

  it('a full reload keeps only the just-sent messages the record does not hold yet', () => {
    const old = [localSend('kept'), localSend('answered')]
    const list = resetEntries(old, [{ id: 'u1', role: 'member', text: 'answered' }])
    expect(states(list)).toEqual([['member', 'answered', 'read'], ['member', 'kept', 'sending']])
  })

  it('a message the terminal kept in its box is marked Not sent, cleared on Send again', () => {
    const bubble = localSend('fix the upload')
    let list = [{ id: 'a0', role: 'companion', text: 'hi' }, bubble]
    expect(stillSending(list, bubble.id)).toBe(true)
    list = markNotSent(list, bubble.id)
    expect(list[1].notSent).toBe(true)
    expect(list[0]).toEqual({ id: 'a0', role: 'companion', text: 'hi' })
    expect(stillSending(list, bubble.id)).toBe(false)
    list = clearNotSent(list, bubble.id)
    expect('notSent' in list[1]).toBe(false)
    expect(stillSending(list, bubble.id)).toBe(true)
  })

  it('only a local bubble is marked, and the record taking it in drops the mark', () => {
    const read = { id: 'u1', role: 'member', text: 'hello', state: 'read' }
    expect(markNotSent([read], 'u1')).toEqual([read])
    let list = markNotSent([localSend('fix the upload')], 'nope')
    expect(list[0].notSent).toBeUndefined()
    const bubble = localSend('fix the upload')
    list = mergeEntries(markNotSent([bubble], bubble.id), [{ id: 'u2', role: 'member', text: 'fix the upload' }])
    expect(states(list)).toEqual([['member', 'fix the upload', 'read']])
    expect(list[0].notSent).toBeUndefined()
    expect(stillSending(list, bubble.id)).toBe(false)
  })
})
