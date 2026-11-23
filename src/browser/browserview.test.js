import { describe, expect, it } from 'vitest'
import { ARTIFACTS, newTabShortcuts, shownAddress } from './browserview.js'

describe('the new-tab page', () => {
  const servers = [
    { port: 5173, folder: '/Users/w/Code/montee', program: 'node' },
    { port: 3000, folder: null, program: 'node' },
    { port: 5173, folder: '/Users/w/Code/montee', program: 'node' },
    { port: 3306, folder: null, program: 'mysqld' },
  ]

  it('shows Artifacts first, then each live web server once (no databases)', () => {
    expect(newTabShortcuts(servers, 'shortcuts')).toEqual([
      { name: 'Artifacts', url: ARTIFACTS, letter: 'A' },
      { name: 'montee :5173', url: 'http://localhost:5173', letter: 'M' },
      { name: 'Server :3000', url: 'http://localhost:3000', letter: 'S' },
    ])
  })

  it('shows nothing on a blank new tab, and copes with no servers', () => {
    expect(newTabShortcuts(servers, 'blank')).toEqual([])
    expect(newTabShortcuts(null, 'shortcuts')).toEqual([{ name: 'Artifacts', url: ARTIFACTS, letter: 'A' }])
  })
})

describe('the address bar', () => {
  it('shows the page in front, or nothing for a new tab', () => {
    expect(shownAddress({ list: [{ id: 2, url: 'https://x.y/' }], active: 2 })).toBe('https://x.y/')
    expect(shownAddress({ list: [{ id: 2, url: null }], active: 2 })).toBe('')
    expect(shownAddress(null)).toBe('')
  })
})
