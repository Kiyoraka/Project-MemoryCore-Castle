import { describe, expect, it } from 'vitest'
import { NO_LIST, contributorsLine, founderEntry, initialsOf, mergedLine, wall } from './wall.js'
import { FOUNDER, PEOPLE } from './people.js'
import counts from './counts.json'

describe('the contributors wall', () => {
  it('joins the counts with the names people asked for, the most first', () => {
    const w = wall({ people: [{ login: 'OneNonly148', merged: 4 }, { login: 'Brian-C69', merged: 49 }] }, PEOPLE)
    expect(w.people.map((p) => [p.name, p.merged, p.initials])).toEqual([['Bernard Choong', 49, 'BC'], ['OneNonly148', 4, 'ON']])
    expect(w.people[0].built).toContain('The Alerts page and its sounds')
    expect([w.total, w.merged, w.hidden]).toEqual([2, 53, 0])
  })

  it('shows a newcomer by login with a thank-you, and never a zero', () => {
    const w = wall({ people: [{ login: 'rin-dev', merged: 1 }, { login: 'ghost', merged: 0 }, { merged: 3 }] }, PEOPLE)
    expect(w.people).toEqual([{ login: 'rin-dev', name: 'rin-dev', merged: 1, built: [NO_LIST], initials: 'RD' }])
  })

  it("fills a newcomer's card from their own merged titles, and a hand-written list still wins", () => {
    const counts = { people: [{ login: 'rin-dev', merged: 2, built: ['Draw with ComfyUI', 'Keep the progress'] }, { login: 'Brian-C69', merged: 9, built: ['Some title'] }] }
    const w = wall(counts, PEOPLE)
    expect(w.people.find((p) => p.login === 'rin-dev').built).toEqual(['Draw with ComfyUI', 'Keep the progress'])
    expect(w.people.find((p) => p.login === 'Brian-C69').built).toContain('The Alerts page and its sounds')
  })

  it('shows eight until Show all', () => {
    const many = { people: Array.from({ length: 11 }, (_, i) => ({ login: `p${String(i).padStart(2, '0')}`, merged: 20 - i })) }
    const first = wall(many, {})
    expect([first.people.length, first.hidden, first.total]).toEqual([8, 3, 11])
    expect(wall(many, {}, { all: true }).people.length).toBe(11)
    expect(wall(null, {})).toEqual({ total: 0, merged: 0, hidden: 0, people: [] })
  })

  it('makes initials and the small lines', () => {
    expect(initialsOf('Bernard Choong')).toBe('BC')
    expect(initialsOf('OneNonly148')).toBe('ON')
    expect(initialsOf('rin')).toBe('R')
    expect(initialsOf('')).toBe('?')
    expect(contributorsLine(1)).toBe('1 contributor')
    expect(contributorsLine(2)).toBe('2 contributors')
    expect(mergedLine(49)).toBe('49 merged')
  })

  it('puts the founder first as Founder, never counted among the contributors', () => {
    const f = founderEntry(FOUNDER)
    expect([f.login, f.founder, f.merged, f.initials]).toEqual(['Kiyoraka', true, 0, 'K'])
    expect(f.built[0]).toBe('Created MemoryCore and MemoryCore Castle')
    expect(founderEntry(null)).toBeNull()
    expect(founderEntry({ login: 'x' }).built).toEqual([])
    expect(wall(counts, PEOPLE).people.some((p) => p.login === 'Kiyoraka')).toBe(false)
  })

  it('reads the packed counts file', () => {
    expect(counts.people.every((p) => p.login && p.merged > 0)).toBe(true)
  })
})
