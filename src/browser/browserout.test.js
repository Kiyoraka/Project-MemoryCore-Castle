import { describe, expect, it } from 'vitest'
import { backWord, outColumns, outHeader } from './browserout.js'

describe("a browser tab's window of its own", () => {
  const list = [
    { id: 1, title: 'Gmail', url: 'https://mail.example/', unread: 4, window: 'home' },
    { id: 2, title: 'Threads', url: 'https://threads.example/', unread: 3, window: 'browser-out-1' },
    { id: 3, title: 'Video', url: 'https://video.example/', unread: 0, window: 'browser-out-2' },
    { id: 4, title: 'A secret research page with a very long name', url: 'https://secret.example/', unread: 1, private: true, window: 'browser-out-2', danger: { url: 'x' } },
  ]

  it('shows a chip for each of its own pages, and the page the address acts on', () => {
    const one = outHeader(list, 'browser-out-1')
    expect(one).toMatchObject({ ids: [2], focus: 2, url: 'https://threads.example/', private: false })
    expect(one.chips).toEqual([{ id: 2, title: 'Threads', full: 'https://threads.example/', unread: 3, private: false, danger: false }])
    const two = outHeader(list, 'browser-out-2', 4)
    expect(two).toMatchObject({ ids: [3, 4], focus: 4, private: true })
    expect(two.chips[1]).toMatchObject({ title: 'A secret research page with…', full: 'Private - https://secret.example/', unread: 1, private: true, danger: true })
  })

  it('falls back to the first page when the one asked for left, and has no header with none left', () => {
    expect(outHeader(list, 'browser-out-2', 99).focus).toBe(3)
    expect(outHeader(list, 'browser-out-9')).toBe(null)
    expect(outHeader(null, 'browser-out-1')).toBe(null)
  })

  it('names the way home after where the browser lives', () => {
    expect(backWord('castle')).toBe('Back to Castle')
    expect(backWord('window')).toBe('Back to the browser')
    expect(backWord(undefined)).toBe('Back to the browser')
  })

  it('lays two to four pages side by side in equal columns', () => {
    expect(outColumns(1)).toBe('1fr')
    expect(outColumns(3)).toBe('1fr 10px 1fr 10px 1fr')
    expect(outColumns(0)).toBe('1fr')
  })
})
