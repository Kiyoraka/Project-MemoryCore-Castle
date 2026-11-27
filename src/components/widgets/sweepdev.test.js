import { describe, expect, it } from 'vitest'
import { clockOf, confirmLine, devResultLine, devSummary, diskWarning, groupSummary, pickedBytes, picksToSend, startingPicks, tickedTotal, worktreeLine } from './sweepdev.js'

const GB = 1024 ** 3
const items = [
  { id: 'target-debug:/c/app/target/debug', kind: 'target-debug', group: 'default', bytes: 6 * GB, on: true },
  { id: 'target-release:/c/app/target/release', kind: 'target-release', group: 'default', bytes: 4 * GB, on: true, blocked: 'in use: rustc' },
  { id: 'pnpm:/s', kind: 'pnpm', group: 'default', bytes: 0, bytesKnown: false, on: true },
  { id: 'worktree:/c/app/worktrees/a', kind: 'worktree', group: 'worktrees', bytes: 2 * GB, wholeBytes: 3 * GB, choices: ['build', 'whole'] },
  { id: 'worktree:/c/app/worktrees/b', kind: 'worktree', group: 'worktrees', bytes: 1 * GB, wholeBytes: 2 * GB, choices: ['build'], dirty: true },
  { id: 'build:/c/app/node_modules', kind: 'build', group: 'projects', bytes: GB, on: false },
]

describe('sweep developer', () => {
  it('starts with what is on and free ticked, every worktree on Leave', () => {
    const picks = startingPicks(items)
    expect(picks['target-debug:/c/app/target/debug']).toBe('clean')
    expect(picks['target-release:/c/app/target/release']).toBe('leave')
    expect(picks['worktree:/c/app/worktrees/a']).toBe('leave')
    expect(picks['build:/c/app/node_modules']).toBe('leave')
  })

  it('counts a pick only when it is offered and not in use', () => {
    expect(pickedBytes(items[3], 'whole')).toBe(3 * GB)
    expect(pickedBytes(items[3], 'build')).toBe(2 * GB)
    expect(pickedBytes(items[4], 'whole')).toBe(0)
    expect(pickedBytes(items[1], 'clean')).toBe(0)
    expect(pickedBytes(items[0], 'leave')).toBe(0)
  })

  it('sends only offered picks and adds them up', () => {
    const picks = { ...startingPicks(items), 'worktree:/c/app/worktrees/a': 'whole', 'worktree:/c/app/worktrees/b': 'whole', 'target-release:/c/app/target/release': 'clean' }
    expect(picksToSend(items, picks)).toEqual([
      { id: 'target-debug:/c/app/target/debug', how: 'clean' },
      { id: 'pnpm:/s', how: 'clean' },
      { id: 'worktree:/c/app/worktrees/a', how: 'whole' },
    ])
    expect(tickedTotal(items, picks)).toBe(9 * GB)
    expect(confirmLine(items, picks)).toBe('Clean 3 items, 9.0 GB and more?')
    expect(confirmLine(items, {})).toBe('Nothing is ticked.')
  })

  it('sums up each group and the section', () => {
    const picks = startingPicks(items)
    expect(groupSummary(items.filter((i) => i.group === 'default'), picks)).toBe('3 items · 6.0 GB ticked · 1 in use or changed')
    expect(groupSummary(items.filter((i) => i.group === 'projects'), picks)).toBe('1 item · 0 B ticked')
    const at = Date.UTC(2026, 10, 26, 6, 52) / 1000
    expect(clockOf(at)).toMatch(/^\d\d:52$/)
    expect(devSummary({ scanned: at, total: 9.4 * GB })).toBe(`Developer · 9.4 GB ready to free · scanned ${clockOf(at)}`)
    expect(devSummary({})).toBe('Developer · not scanned yet')
    expect(devSummary({ scanning: true })).toBe('Developer · scanning…')
  })

  it('says a worktree plainly', () => {
    const lastCommit = new Date(2026, 10, 2, 12).getTime() / 1000
    expect(worktreeLine({ branch: 'feat/selagro', mergedInto: 'main', lastCommit })).toBe('branch feat/selagro · merged into main · last commit 2 Nov')
    expect(worktreeLine({ branch: 'feat/x', pushed: true, detail: 'pushed, not merged · build output only' })).toBe('branch feat/x · pushed, not merged · build output only')
    expect(worktreeLine({})).toBe('no branch')
  })

  it('says what was freed and what stayed', () => {
    expect(devResultLine([{ ok: true, freed: 2 * GB }, { ok: true, freed: GB }])).toBe('Freed 3.0 GB from 2 items')
    expect(devResultLine([{ ok: true, freed: GB }, { ok: false, note: 'in use: node' }])).toBe('Freed 1.0 GB from 1 item - 1 stayed (in use: node)')
  })

  it('warns at 90% full on the system drive', () => {
    expect(diskWarning([{ mount: '/', total: 100, free: 8 }])).toEqual({ percent: 92, line: 'Disk 92% full' })
    expect(diskWarning([{ mount: '/', total: 100, free: 11 }])).toBeNull()
    expect(diskWarning([{ mount: 'D:\\', total: 100, free: 1 }, { mount: 'C:\\', total: 100, free: 50 }])).toBeNull()
    expect(diskWarning([])).toBeNull()
  })
})
