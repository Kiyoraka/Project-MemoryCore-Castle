import { describe, expect, it } from 'vitest'
import { ARTIFACTS, newTabShortcuts, shownAddress, signInHint, siteWarning } from './browserview.js'

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

describe('the sign-in hint (F11)', () => {
  it('shows on the claude.ai sign-in page and Google accounts', () => {
    expect(signInHint('https://claude.ai/login?returnTo=%2Fcode%2Fartifacts')).toMatch(/email code/)
    expect(signInHint('https://accounts.google.com/v3/signin/challenge')).toMatch(/email code/)
  })

  it('stays away from other pages, new tabs and look-alike hosts', () => {
    expect(signInHint('https://claude.ai/code/artifacts')).toBe('')
    expect(signInHint('https://claude.ai.evil.example/login')).toBe('')
    expect(signInHint('http://claude.ai/login')).toBe('')
    expect(signInHint('')).toBe('')
    expect(signInHint(null)).toBe('')
  })
})

describe('the site check on the toolbar', () => {
  it('says what the Sentinel found, and nothing for a safe site', () => {
    expect(siteWarning({ verdict: 'dangerous', reason: 'it carries the name paypal but is not paypal\'s own site' })).toBe(
      "The Sentinel marked this site dangerous: it carries the name paypal but is not paypal's own site",
    )
    expect(siteWarning({ verdict: 'suspicious', reason: 'it is a bare address, not a named site' })).toMatch(/^The Sentinel marked this site suspicious: /)
    expect(siteWarning({ verdict: 'safe', reason: 'fine' })).toBe('')
    expect(siteWarning(null)).toBe('')
  })
})
