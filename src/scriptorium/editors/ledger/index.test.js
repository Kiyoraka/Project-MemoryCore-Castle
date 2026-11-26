import { describe, expect, it } from 'vitest'
import ledger from './index.js'
import { editorUi, intentFor } from '../../ribbon.js'
import { INTENTS } from '../../input.js'
import { ICON_NAMES } from '../../icons.js'

// The commands the Rust side lists (scriptorium/src/ledger: command_specs).
const COMMANDS = [
  'ledger.set_cell', 'ledger.set_cells', 'ledger.scroll_to', 'ledger.move', 'ledger.enter', 'ledger.tab', 'ledger.select_all',
  'ledger.insert_text', 'ledger.backspace', 'ledger.clear', 'ledger.fill', 'ledger.autofill', 'ledger.cut', 'ledger.paste',
  'ledger.move_range', 'ledger.autosum', 'ledger.format', 'ledger.toggle', 'ledger.borders', 'ledger.format_painter',
  'ledger.copy_format', 'ledger.merge', 'ledger.unmerge', 'ledger.insert', 'ledger.delete', 'ledger.row_height',
  'ledger.col_width', 'ledger.hide', 'ledger.sort', 'ledger.filter', 'ledger.clear_filter', 'ledger.remove_filter',
  'ledger.find', 'ledger.replace', 'ledger.freeze', 'ledger.unfreeze', 'ledger.add_sheet', 'ledger.rename_sheet',
  'ledger.delete_sheet', 'ledger.move_sheet', 'ledger.import_csv',
]

const allButtons = (ui) => [...ui.tabs, ...ui.panels].flatMap((t) => t.groups.flatMap((g) => g.buttons))

describe('Ledger ribbon', () => {
  it('keeps its tabs and fills them', () => {
    const ui = editorUi('ledger', COMMANDS)
    expect(ui.tabs.map((t) => t.name)).toEqual(['Home', 'Insert', 'Formulas', 'Data', 'View'])
    for (const t of ui.tabs) expect(t.groups.length).toBeGreaterThan(0)
    expect(ui.panels.map((p) => p.id)).toEqual(['cell', 'alignment', 'number', 'borders', 'structure', 'data'])
  })
  it('names only commands the document knows, with icons it can draw', () => {
    const ui = editorUi('ledger', COMMANDS)
    for (const btn of allButtons(ui)) {
      expect(btn.ready, btn.title).toBe(true)
      if (btn.command) expect(COMMANDS, btn.title).toContain(btn.command)
      expect(btn.icon.startsWith('<') || ICON_NAMES.includes(btn.icon), btn.title).toBe(true)
    }
  })
  it('answers the key intents with its own commands', () => {
    for (const name of Object.keys(ledger.intents)) {
      expect(INTENTS).toContain(name)
      expect(COMMANDS).toContain(ledger.intents[name].command)
    }
    expect(intentFor('ledger', 'caret.down')).toEqual({ command: 'ledger.move', args: { motion: 'down' } })
    expect(intentFor('ledger', 'edit.delete')).toEqual({ command: 'ledger.clear', args: { what: 'contents' } })
    expect(intentFor('ledger', 'format.bold')).toEqual({ command: 'ledger.toggle', args: { what: 'bold' } })
  })
})
