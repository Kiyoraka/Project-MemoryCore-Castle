import { describe, expect, it } from 'vitest'
import { HELPER_LINE, helperWord, lockCall, readGames, readState, recipeWord, shotWord, stepsLine, tagPlaces } from './worldsmith.js'

describe('WORLDSMITH page helpers', () => {
  it('lists games with the refused ones last and the remembered ones first', () => {
    const games = readGames(
      {
        games: [
          { pid: 3, exe: 'Shooter.exe', title: 'Shooter', refused: true, reason: 'This one runs anti-cheat (EasyAntiCheat).' },
          { pid: 1, exe: 'Moss.exe', title: 'Moss Kingdom', stamp: '10-1' },
          { pid: 2, exe: 'Ember.exe', title: 'Ember Quest', stamp: '20-2' },
          { pid: 0, exe: 'broken.exe' },
        ],
      },
      [{ exe: 'ember.exe', stamp: '20-2' }],
    )
    expect(games.map((g) => g.title)).toEqual(['Ember Quest', 'Moss Kingdom', 'Shooter'])
    expect(games[0].remembered).toBe(true)
    expect(games[2]).toMatchObject({ refused: true, reason: 'This one runs anti-cheat (EasyAntiCheat).' })
    expect(readGames(null)).toEqual([])
  })

  it('reads the state into tags with their steps, address and lock', () => {
    const s = readState({
      attached: { pid: 2, exe: 'Ember.exe' },
      labels: [
        { label: 'Gold', steps: [1284, 3, 1], top: [{ address: '0x1a2f40', type: 'i32', value: '1234' }] },
        { label: 'Health', steps: [5120], top: [] },
      ],
      freezes: [{ address: '0x1A2F40', type: 'i32', value: '1234' }],
    })
    expect(s.labels[0]).toMatchObject({ label: 'Gold', found: true, address: '0x1A2F40', locked: true, type: 'i32' })
    expect(s.labels[1]).toMatchObject({ found: false, address: '', locked: false })
    expect(stepsLine([1284, 3, 1])).toBe('1,284 → 3 → 1')
  })

  it('pins tags where the companion placed them and lines the rest up', () => {
    const tags = tagPlaces([{ label: 'Gold' }, { label: 'Ammo' }, { label: 'Health' }], [{ label: 'Gold', pinX: 0.56, pinY: 0.12 }, { label: 'Health', pinX: 2, pinY: -1 }])
    expect(tags[0]).toMatchObject({ x: 0.56, y: 0.12, placed: true })
    expect(tags[1]).toMatchObject({ x: 0.03, y: 0.06, placed: false })
    expect(tags[2]).toMatchObject({ x: 0.97, y: 0.03, placed: true })
  })

  it('locks and unlocks through the helper, never without an address', () => {
    expect(lockCall({ address: '0x1A2F40', type: 'i32', value: 1234, locked: false })).toEqual(['freeze', ['0x1A2F40', 'i32', '1234']])
    expect(lockCall({ address: '0x1A2F40', type: 'i32', value: 1234, locked: true })).toEqual(['unfreeze', ['0x1A2F40']])
    expect(lockCall({ address: '', type: 'i32' })).toBe(null)
  })

  it('says the plain words for the helper and the session', () => {
    expect(helperWord({ windows: false })).toBe('WORLDSMITH works on Windows only.')
    expect(helperWord({ windows: true, installed: true })).toBe('')
    expect(helperWord({ windows: true, installed: false, released: true })).toBe(HELPER_LINE)
    expect(HELPER_LINE).toContain('offline single-player')
    expect(shotWord('C:\\t\\shot.png')).toBe('Here is a new screenshot of the game: C:\\t\\shot.png')
    expect(recipeWord({ name: 'Gold to 9,999', action: 'write', labels: 'Gold', amount: '9999' })).toBe('Please use the recipe "Gold to 9,999" (write Gold to 9999).')
  })
})
