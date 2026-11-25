import { describe, expect, it } from 'vitest'
import { ENTER, ENTER_DELAY_MS, enterDelay, typeLine } from './typeline.js'

describe('typing a line into the session', () => {
  it('types the text, pauses, then presses Enter - in that order', async () => {
    const steps = []
    await typeLine(
      async (data) => steps.push(['write', data]),
      'save diary',
      async (ms) => steps.push(['wait', ms]),
    )
    expect(steps).toEqual([['write', 'save diary'], ['wait', ENTER_DELAY_MS], ['write', ENTER]])
  })

  it('pauses long enough for Codex, which ignores an Enter within about 50 ms of the text', () => {
    expect(ENTER_DELAY_MS).toBeGreaterThanOrEqual(100)
    expect(ENTER).toBe('\r')
  })

  it('waits 700 ms more for each picture path, counted inside a bracketed paste too', async () => {
    expect(enterDelay('save diary')).toBe(300)
    expect(enterDelay('see 3.png and the docs/a.png page')).toBe(300)
    expect(enterDelay('look C:\\shots\\one.PNG and "D:\\My Pictures\\two.jpeg" please')).toBe(1700)
    const text = '\u001b[200~/home/me/a.webp\nwhat is it\u001b[201~'
    expect(enterDelay(text)).toBe(1000)
    const waits = []
    await typeLine(async () => {}, text, async (ms) => waits.push(ms))
    expect(waits).toEqual([1000])
  })

  it('does not press Enter when the text could not be written', async () => {
    const written = []
    const write = async (data) => {
      if (data === 'save diary') throw new Error('no session')
      written.push(data)
    }
    await expect(typeLine(write, 'save diary', async () => {})).rejects.toThrow('no session')
    expect(written).toEqual([])
  })
})
