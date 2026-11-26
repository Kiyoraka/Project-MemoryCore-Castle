import { describe, expect, it } from 'vitest'
import { doorDone, formatForPath, nativeOnly, readRequest, takeOnce } from './photoDoor.js'

describe('PHOTO and DEV (Phase 235)', () => {
  it('saves a picture back in its own format', () => {
    expect(formatForPath('D:\\Work\\Kopi\\logo.PNG')).toBe('png')
    expect(formatForPath('a/b/banner.jpg')).toBe('jpeg')
    expect(formatForPath('x.jpeg')).toBe('jpeg')
    expect(formatForPath('x.webp')).toBe('webp')
    expect(formatForPath('notes.txt')).toBe(null)
    expect(formatForPath(null)).toBe(null)
  })

  it('refuses a redraw on a door edit and keeps her reading', async () => {
    const ask = async () => 'read'
    const api = nativeOnly({ photoAsk: ask, photoGenerate: async () => 'drawn' })
    expect(api.photoAsk).toBe(ask)
    await expect(api.photoGenerate()).rejects.toThrow('only the native tools')
  })

  it('answers the terminal with her words or the reason', () => {
    expect(doorDone({ id: 'p-1', file: 'f.png' }, { ok: true, say: 'Cropped it square.' })).toEqual({ id: 'p-1', file: 'f.png', ok: true, say: 'Cropped it square.', reason: null })
    expect(doorDone({ id: 'p-1', file: 'f.png' }, { ok: false, reason: 'busy' }).reason).toBe('busy')
    expect(doorDone({ id: 'p-1', file: 'f.png' }, null).ok).toBe(false)
  })

  it('acts only on a picture, and on a door with an id and words', () => {
    expect(readRequest({ kind: 'open', path: 'a.png', saveBack: 1 })).toEqual({ kind: 'open', path: 'a.png', saveBack: true })
    expect(readRequest({ kind: 'open', path: 'a.txt' })).toBe(null)
    expect(readRequest({ kind: 'door', path: 'a.jpg', id: 'p-2', say: ' crop it ' })).toEqual({ kind: 'door', path: 'a.jpg', id: 'p-2', say: 'crop it' })
    expect(readRequest({ kind: 'door', path: 'a.jpg', id: '../x', say: 'crop' })).toBe(null)
    expect(readRequest({ kind: 'door', path: 'a.jpg', id: 'p-2', say: '  ' })).toBe(null)
    expect(readRequest(null)).toBe(null)
  })

  it('handles each request once, even after PHOTO opens again', () => {
    expect(takeOnce(111)).toBe(true)
    expect(takeOnce(111)).toBe(false)
    expect(takeOnce(222)).toBe(true)
    expect(takeOnce(undefined)).toBe(false)
  })
})
