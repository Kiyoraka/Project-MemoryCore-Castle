import { describe, expect, it } from 'vitest'
import { roomPresenceState } from './roomchat.js'

describe('room presence is explicit and respects companion admission', () => {
  const me = { name: 'Bernard', kind: 'member' }
  const own = { name: "Bernard's Mira", kind: 'member_assistant', status: 'online' }
  it('overrides a stale active row when the own companion is disabled, without changing the raw row', () => {
    const row = own
    expect(roomPresenceState(row, { me, fresh: true, enabled: false })).toBe('disabled')
    expect(roomPresenceState(row, { me, fresh: true, enabled: true, verified: true })).toBe('online')
    expect(roomPresenceState(row, { me, fresh: true, enabled: true, verified: false })).toBe('unknown')
    expect(own.status).toBe('online')
  })
  it('uses verified identity for renamed companions and never disables other owners or humans', () => {
    const renamed = { name: 'Mira', kind: 'member_assistant', status: 'online' }
    expect(roomPresenceState(renamed, { companion: renamed, fresh: true })).toBe('disabled')
    expect(roomPresenceState({ ...own, name: "Rin's Mira" }, { me, fresh: true })).toBe('online')
    expect(roomPresenceState({ ...me, status: 'online' }, { me, fresh: true })).toBe('online')
    expect(roomPresenceState({ ...own, name: 'Bernard\u2019s Mira' }, { me, fresh: true })).toBe('disabled')
    expect(roomPresenceState({ ...own, name: 'Mira', ownerName: 'Bernard' }, { me, fresh: true })).toBe('disabled')
  })
  it('treats offline, idle, missing, unfamiliar and failed-refresh statuses as inactive', () => {
    for (const status of ['offline', 'idle', 'away']) expect(roomPresenceState({ status }, { fresh: true })).toBe(status)
    for (const status of [undefined, 'unexpected']) expect(roomPresenceState({ status }, { fresh: true })).toBe('unknown')
    expect(roomPresenceState(own, { me, enabled: true, fresh: false })).toBe('unknown')
    for (const status of ['online', 'here', 'active']) expect(roomPresenceState({ status }, { fresh: true })).toBe(status)
  })
})

describe('the room says a member AI by her own name, owner apart (Nov 14 2026)', () => {
  it('knows the own companion by owner and shows the owner on hover', async () => {
    const { roomPresenceState: state, speakerPresentation } = await import('./roomchat.js')
    const me = { name: 'Bernard', kind: 'member' }
    const mira = { name: 'Mira', owner: 'Bernard', kind: 'member_assistant', status: 'online' }
    expect(state(mira, { me, fresh: true, enabled: false })).toBe('disabled')
    expect(state({ ...mira, owner: 'Rin' }, { me, fresh: true })).toBe('online')
    expect(speakerPresentation(mira)).toEqual({ name: 'Mira', owner: 'Bernard', title: 'Master: Bernard' })
  })
})
