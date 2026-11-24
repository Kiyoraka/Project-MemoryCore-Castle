import { describe, expect, it } from 'vitest'
import { BUILT_KEPT, countMerged, countsFile, countsLine, plainTitle, smallAvatar } from './contributors-tools.mjs'

const pull = (login, merged, extra = {}) => ({ merged_at: merged ? '2026-11-19T10:00:00Z' : null, user: { login, type: 'User', avatar_url: `https://avatars.githubusercontent.com/u/${login.length}?v=4`, ...extra } })

describe('the contributors wall', () => {
  it('counts merged pull requests per person, the most first', () => {
    const people = countMerged([pull('Brian-C69', true), pull('OneNonly148', true), pull('Brian-C69', true), pull('Brian-C69', false)])
    expect(people.map(({ login, merged }) => [login, merged])).toEqual([['Brian-C69', 2], ['OneNonly148', 1]])
    expect(people[0].avatar).toContain('avatars.githubusercontent.com')
  })

  it('leaves out the owner, bots and pulls that were never merged', () => {
    const people = countMerged([pull('Kiyoraka', true), pull('kiyoraka', true), pull('dependabot[bot]', true), pull('helper', true, { type: 'Bot' }), pull('Rin', false), { merged_at: 'x' }])
    expect(people).toEqual([])
  })

  it('orders a tie by login and builds the file the page reads', () => {
    const people = countMerged([pull('zed', true), pull('amy', true)])
    expect(people.map((p) => p.login)).toEqual(['amy', 'zed'])
    const file = countsFile([{ login: 'Brian-C69', merged: 49, avatar: 'x' }, { login: 'OneNonly148', merged: 4 }], '2026-11-19T16:00:00Z')
    expect(file).toEqual({ at: '2026-11-19T16:00:00Z', merged: 53, people: [{ login: 'Brian-C69', merged: 49 }, { login: 'OneNonly148', merged: 4 }] })
    expect(countsLine(file)).toBe('Contributors: 2 contributors, 53 merged')
    expect(countsLine(countsFile([{ login: 'a', merged: 1 }], 'x'))).toBe('Contributors: 1 contributor, 1 merged')
  })

  it('says what each person built, from their own merged titles, newest first', () => {
    const titled = (login, title, at) => ({ ...pull(login, true), title, merged_at: at })
    const people = countMerged([
      titled('NecroSlav', 'fix: keep the install progress', '2026-11-20T10:00:00Z'),
      titled('NecroSlav', "feat: draw STUDIO's plain pictures with your own ComfyUI", '2026-11-24T10:00:00Z'),
      titled('NecroSlav', 'feat(db): carry the download on (stacked on #83)', '2026-11-23T10:00:00Z'),
      { ...pull('NecroSlav', false), title: 'feat: never merged' },
    ])
    expect(people[0].built).toEqual(["Draw STUDIO's plain pictures with your own ComfyUI", 'Carry the download on', 'Keep the install progress'])
    const many = countMerged(Array.from({ length: 9 }, (_, i) => titled('Rin', `fix: thing ${i}`, `2026-11-1${i}T00:00:00Z`)))
    expect(many[0].built).toHaveLength(BUILT_KEPT)
    expect(countsFile(people, 'x').people[0].built).toHaveLength(3)
    expect(countsFile([{ login: 'a', merged: 1, built: [] }], 'x').people[0]).toEqual({ login: 'a', merged: 1 })
  })

  it('turns a pull request title into a plain line', () => {
    expect(plainTitle('feat: say Install will continue')).toBe('Say Install will continue')
    expect(plainTitle('fix(resume)!: a short answer')).toBe('A short answer')
    expect(plainTitle('APPS search: one focus ring instead of two')).toBe('APPS search: one focus ring instead of two')
    expect(plainTitle('APPS: a room launcher (stacked on #83 and #84)')).toBe('APPS: a room launcher')
    expect(plainTitle('')).toBe('')
    expect(plainTitle('x'.repeat(120)).length).toBe(90)
  })

  it('asks for a small avatar', () => {
    expect(smallAvatar('https://avatars.githubusercontent.com/u/1?v=4')).toBe('https://avatars.githubusercontent.com/u/1?v=4&s=96')
    expect(smallAvatar('https://example.com/a.png', 64)).toBe('https://example.com/a.png?s=64')
  })
})
