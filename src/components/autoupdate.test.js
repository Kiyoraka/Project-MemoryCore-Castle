import { describe, expect, it } from 'vitest'
import { mayUpdateByItself } from './autoupdate.js'

const ready = { available: '0.9.0', canInstall: true }

describe('update by itself', () => {
  it('starts only when switched on, an update can install, the companion is quiet and nothing runs', () => {
    expect(mayUpdateByItself({ status: ready, on: true, busy: false, step: '' })).toBe(true)
  })

  it('stays off by default', () => {
    expect(mayUpdateByItself({ status: ready, on: false, busy: false, step: '' })).toBe(false)
    expect(mayUpdateByItself({ status: ready, busy: false, step: '' })).toBe(false)
  })

  it('never interrupts the companion mid-answer', () => {
    expect(mayUpdateByItself({ status: ready, on: true, busy: true, step: '' })).toBe(false)
  })

  it('never starts a second time while an update is under way', () => {
    expect(mayUpdateByItself({ status: ready, on: true, busy: false, step: 'Downloading the update…' })).toBe(false)
  })

  it('leaves a member-built copy alone (it updates with git pull)', () => {
    expect(mayUpdateByItself({ status: { available: '0.9.0', canInstall: false }, on: true, busy: false, step: '' })).toBe(false)
    expect(mayUpdateByItself({ status: { available: null, canInstall: true }, on: true, busy: false, step: '' })).toBe(false)
    expect(mayUpdateByItself({ status: null, on: true, busy: false, step: '' })).toBe(false)
  })
})
