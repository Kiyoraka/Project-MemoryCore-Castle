import { describe, expect, it } from 'vitest'
import { doneLine, nextStep, offeredStep, stepNumber, stepWords } from './browser_welcome.js'

describe('the browser welcome, one step at a time', () => {
  it('offers the first step with no answer, and nothing once all are answered', () => {
    expect(nextStep(undefined)).toBe('history')
    expect(nextStep({ history: 'yes' })).toBe('bookmarks')
    expect(nextStep({ history: 'skip', bookmarks: 'yes' })).toBe('passwords')
    expect(nextStep({ history: 'yes', bookmarks: 'skip', passwords: 'skip', sitecheck: 'yes' })).toBeNull()
    expect(stepNumber('passwords')).toBe(3)
  })

  it('without Chrome on this PC only the site check is offered', () => {
    expect(offeredStep({}, false)).toBe('sitecheck')
    expect(offeredStep({ sitecheck: 'skip' }, false)).toBeNull()
    expect(offeredStep({}, true)).toBe('history')
  })

  it('a skip is never offered again; Settings clearing it offers it back', () => {
    const answers = { history: 'skip', bookmarks: 'skip', passwords: 'skip', sitecheck: 'skip' }
    expect(nextStep(answers)).toBeNull()
    const reopened = Object.fromEntries(Object.entries(answers).filter(([, a]) => a !== 'skip'))
    expect(nextStep(reopened)).toBe('history')
  })

  it('each step says what is read and what never is, in the companion\'s name', () => {
    expect(stepWords('history', 'Shana').body).toContain('Never what you typed')
    const pw = stepWords('passwords', 'Shana')
    expect(pw.body).toContain('never sees a password')
    expect(pw.body).toContain('Windows PIN')
    expect(pw.guide).toBe('Ask Shana to guide me')
    expect(stepWords('sitecheck', '').title).toBe('Let your companion check sites for you?')
    expect(stepWords('nothing')).toBeNull()
  })

  it('the done lines are counts only', () => {
    expect(doneLine('history', 1)).toBe('1 page of history brought over.')
    expect(doneLine('bookmarks', 12)).toBe('12 bookmarks brought over.')
    expect(doneLine('passwords', { sealed: 42, skipped: 0, shredded: true })).toBe('42 passwords sealed in your vault; the export file is gone.')
    expect(doneLine('passwords', { sealed: 1, skipped: 0, shredded: false })).toContain('delete it yourself now')
    expect(doneLine('passwords', null)).toBe('')
  })
})
