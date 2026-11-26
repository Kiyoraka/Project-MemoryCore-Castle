import { describe, expect, it } from 'vitest'
import { parseBlocks } from './markdown.js'
import {
  addColumn,
  addRow,
  cellText,
  fromTable,
  isTicked,
  message,
  removeColumn,
  removeRow,
  renameColumn,
  selectAll,
  selectedCount,
  setCell,
  setCheck,
  setTickName,
  toMarkdown,
  toggleSelected,
  toggleTicked,
} from './chatTable.js'

const WALK = '| Walkthrough | Desc | Status |\n|---|---|---|\n| W1 | About | X |\n| W2 | Wake | |\n| W3 | HOME | X |'
const model = () => {
  const [t] = parseBlocks(WALK)
  return fromTable(t.head, t.rows)
}

describe('chat table model', () => {
  it('reads a chat table into columns and rows, padding short rows', () => {
    const m = fromTable(['A', 'B'], [['1'], ['2', '3', 'extra']])
    expect(m.columns.map((c) => c.name)).toEqual(['A', 'B', ''])
    expect(m.rows[0].cells).toEqual(['1', '', ''])
    expect(m.rows[1].cells).toEqual(['2', '3', 'extra'])
    expect(m.ticking).toBe(false)
  })

  it('writes the table back as markdown that parses to the same table', () => {
    const m = model()
    const [again] = parseBlocks(toMarkdown(m))
    expect(again.head).toEqual(['Walkthrough', 'Desc', 'Status'])
    expect(again.rows).toEqual([['W1', 'About', 'X'], ['W2', 'Wake', ''], ['W3', 'HOME', 'X']])
  })

  it('edits cells, rows and columns without touching the model it came from', () => {
    const m = model()
    let e = setCell(m, 1, 1, 'Wake, resume')
    e = addRow(e)
    e = setCell(e, 3, 0, 'W4')
    e = addColumn(e, 'Note', 1)
    e = renameColumn(e, 1, 'Notes')
    e = removeRow(e, 0)
    expect(m.rows[1].cells[1]).toBe('Wake')
    expect(e.columns.map((c) => c.name)).toEqual(['Walkthrough', 'Notes', 'Desc', 'Status'])
    expect(e.rows.map((r) => r.cells[0])).toEqual(['W2', 'W3', 'W4'])
    expect(removeColumn(e, 1).columns).toHaveLength(3)
    expect(removeColumn(fromTable(['Only'], []), 0).columns).toHaveLength(1)
  })

  it('a column switched to ticks reads X / ✓ / done as ticked and writes [x] / [ ]', () => {
    const m = setCheck(model(), 2, true)
    expect(m.rows.map((r) => r.cells[2])).toEqual([true, false, true])
    expect(toMarkdown(m).split('\n')[3]).toBe('| W2 | Wake | [ ] |')
    expect(addRow(m).rows.at(-1).cells[2]).toBe(false)
    expect(setCheck(m, 2, false).rows.map((r) => r.cells[2])).toEqual(['x', '', 'x'])
    for (const yes of ['X', '[x]', '✓', 'done', 'Yes']) expect(isTicked(yes), yes).toBe(true)
    for (const no of ['', 'no', 'later', '[ ]']) expect(isTicked(no), no).toBe(false)
  })

  it('sends only the picked rows, after the edits', () => {
    let m = toggleSelected(model(), 0)
    m = toggleSelected(m, 2)
    m = setCell(m, 2, 1, 'HOME cards')
    expect(selectedCount(m)).toBe(2)
    expect(message(m, '', 'selected')).toBe('| Walkthrough | Desc | Status |\n| --- | --- | --- |\n| W1 | About | X |\n| W3 | HOME cards | X |')
    expect(selectedCount(selectAll(m))).toBe(3)
    expect(selectedCount(selectAll(selectAll(m)))).toBe(0)
  })

  it('ticks answer rows: the whole table goes with a Done column', () => {
    let m = toggleTicked(model(), 0)
    m = toggleTicked(m, 2)
    m = setTickName(m, 'Tested')
    const lines = message(m, 'W1 and W3 pass; W2 not yet.').split('\n')
    expect(lines[0]).toBe('W1 and W3 pass; W2 not yet.')
    expect(lines[2]).toBe('| Walkthrough | Desc | Status | Tested |')
    expect(lines.slice(4)).toEqual(['| W1 | About | X | [x] |', '| W2 | Wake |  | [ ] |', '| W3 | HOME | X | [x] |'])
  })

  it('context alone sends the table unchanged with the words; nothing picked sends only the words', () => {
    expect(message(model(), 'Looks right')).toBe(`Looks right\n\n${toMarkdown(model())}`)
    expect(message(model(), 'Only this', 'selected')).toBe('Only this')
    expect(message(model(), '', 'selected')).toBe('')
  })

  it('a cell with a pipe or a line break stays one cell', () => {
    expect(cellText('a | b\nc')).toBe('a \\| b c')
    const m = setCell(model(), 0, 1, 'About | version')
    const [again] = parseBlocks(toMarkdown(m))
    expect(again.rows[0]).toEqual(['W1', 'About | version', 'X'])
  })
})
