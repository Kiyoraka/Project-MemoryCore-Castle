import { describe, expect, it } from 'vitest'
import { card, thisComputer, installButton, installLine, megabytes, moveLine, nextStep, progressLine, setupStepLabel, shouldOffer, steps } from './database.js'

const none = { state: 'notInstalled', external: null }
const stopped = { state: 'stopped', port: 3307 }
const running = { state: 'running', port: 3307 }
const xampp = (answering) => ({ state: 'external', port: 3306, answering })
const empty = { name: null, reachable: false, movedIn: false, canRestore: false }
const made = { name: 'lyra_memorycoredb', reachable: true, movedIn: false, canRestore: false }
const home = { ...made, movedIn: true }

describe('the database steps', () => {
  it('walks install, start, create, move in order', () => {
    expect(nextStep(none, empty)).toBe('install')
    expect(nextStep(stopped, empty)).toBe('start')
    expect(nextStep(running, empty)).toBe('create')
    expect(nextStep(running, made)).toBe('move')
    expect(nextStep(running, home)).toBe(null)
  })

  it('never counts a database as created or moved in while the engine is stopped', () => {
    // the .env still names it, but nothing answers
    expect(nextStep(stopped, { ...home, reachable: false })).toBe('start')
    expect(steps(stopped, home).map((s) => s.ok)).toEqual([true, false, false, false])
  })

  it('treats an existing server as installed, and running only while it answers', () => {
    expect(steps(xampp(true), made)[0].label).toBe('Using the database already on this PC')
    expect(steps(xampp(true), made, false)[0].label).toBe('Using the database already on this PC')
    expect(steps(xampp(true), made, true)[0].label).toBe('Using the database already on this Mac')
    expect(card(xampp(false), made, true).line).toBe('The database on this Mac is stopped.')
    expect(card(xampp(false), made, false).line).toBe('The database on this PC is stopped.')
    expect(thisComputer(true)).toBe('this Mac')
    expect(thisComputer(false)).toBe('this PC')
    expect(nextStep(xampp(false), made)).toBe('start')
    expect(nextStep(xampp(true), made)).toBe('move')
  })

  it('names the database once it exists', () => {
    expect(steps(running, made)[2].label).toBe('lyra_memorycoredb created')
    expect(steps(running, empty)[2].label).toBe('Your memory database created')
  })

  it('still answers before the core has', () => {
    expect(nextStep(null, null)).toBe('install')
  })
})

describe('the words', () => {
  it('rounds the download size and the progress', () => {
    expect(megabytes(95024126)).toBe('91 MB')
    expect(installLine({ source: 'download', downloadBytes: 95024126 })).toBe('About 91 MB')
    expect(installLine({ source: 'homebrew', downloadBytes: 95024126 })).toBe('Installed with Homebrew')
    expect(installLine({ source: 'download', downloadBytes: 95024126, partialBytes: 26214400 })).toBe('25 MB of 91 MB already downloaded')
    expect(installLine({ source: 'download', downloadBytes: 95024126, partialBytes: 0 })).toBe('About 91 MB')
    expect(installButton({ source: 'download', downloadBytes: 95024126, partialBytes: 26214400 })).toBe('Continue the download')
    expect(installButton({ source: 'download', downloadBytes: 95024126, partialBytes: 0 })).toBe('Install the database')
    expect(installButton({ source: 'homebrew', partialBytes: 5 })).toBe('Install the database')
    expect(installButton(null)).toBe('Install the database')
    expect(progressLine(0, 0)).toBe('Downloading…')
    expect(progressLine(41, 95)).toBe('Downloading… 43%')
    expect(progressLine(200, 95)).toBe('Downloading… 100%')
  })

  it('says what will move, in plain counts', () => {
    expect(moveLine({ files: 14, diary: 6, memory: 4, reminders: 3, projects: 3 })).toBe('14 files will move: 6 diary pages, 4 memory files, 3 reminders, 3 projects.')
    expect(moveLine({ files: 1, diary: 1, memory: 0, reminders: 0, projects: 0 })).toBe('1 file will move: 1 diary page.')
    expect(moveLine({ files: 0, diary: 0, memory: 0, reminders: 0, projects: 0 })).toMatch(/nothing to move/)
    expect(moveLine(null)).toBe('')
  })
})

describe('the HOME card', () => {
  it('offers the one thing to do next', () => {
    expect(card(null, null)).toEqual({ line: 'Checking…', button: null })
    expect(card(none, empty).button).toEqual({ label: 'Set up', action: 'open' })
    expect(card(stopped, made)).toEqual({ line: 'Stopped.', button: { label: 'Start', action: 'start' } })
    expect(card(running, made).line).toBe('Running · your memory is still in files.')
    expect(card(running, home)).toEqual({ line: 'lyra_memorycoredb · running', button: null })
  })

  it('never offers to start a server Castle does not own', () => {
    expect(card(xampp(false), made).button.action).toBe('open')
  })
})

describe('the one-time offer after an update', () => {
  const member = { offerSeen: false, hadCastleBefore: true, hasFolder: true, setupOpen: false }

  it('is shown to a member who updated and whose memory is still in files', () => {
    expect(shouldOffer(member, none, empty)).toBe(true)
  })

  it('is asked once: either button marks it seen', () => {
    expect(shouldOffer({ ...member, offerSeen: true }, none, empty)).toBe(false)
  })

  it('never shows on a first run, during setup, or before a memory folder is chosen', () => {
    expect(shouldOffer({ ...member, hadCastleBefore: false }, none, empty)).toBe(false)
    expect(shouldOffer({ ...member, setupOpen: true }, none, empty)).toBe(false)
    expect(shouldOffer({ ...member, hasFolder: false }, none, empty)).toBe(false)
  })

  it('never shows once any database is there, or before the core has answered', () => {
    expect(shouldOffer(member, stopped, empty)).toBe(false)
    expect(shouldOffer(member, running, made)).toBe(false)
    expect(shouldOffer(member, xampp(true), empty)).toBe(false)
    expect(shouldOffer(member, none, home)).toBe(false)
    expect(shouldOffer(member, null, null)).toBe(false)
  })

  it('names each step as it runs', () => {
    expect(setupStepLabel('install')).toBe('Downloading and installing the database')
    expect(setupStepLabel('check')).toBe('Checking what would move')
    expect(setupStepLabel('other')).toBe('other')
  })
})
