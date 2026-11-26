import { describe, expect, it } from 'vitest'
import scroll from './index.js'
import { INSERT_INTENTS, linkCommand, pictureCommand, toBase64 } from './insert.js'
import { editorUi, intentFor } from '../../ribbon.js'
import { INTENTS } from '../../input.js'
import { ICON_NAMES } from '../../icons.js'

// The commands the Rust side lists (scriptorium/src/scroll/mod.rs: commands + write_specs).
const COMMANDS = [
  'scroll.set_title', 'scroll.set_paragraph_text', 'scroll.insert_text', 'scroll.delete', 'scroll.enter', 'scroll.tab', 'scroll.paste',
  'scroll.format', 'scroll.toggle', 'scroll.font_size_step', 'scroll.clear_format', 'scroll.paragraph', 'scroll.align', 'scroll.indent',
  'scroll.style', 'scroll.list', 'scroll.list_level', 'scroll.list_restart', 'scroll.find', 'scroll.replace', 'scroll.select',
  'scroll.insert_table', 'scroll.table_insert', 'scroll.table_delete', 'scroll.merge_cells', 'scroll.table_widths', 'scroll.row_height',
  'scroll.row_break', 'scroll.header_rows', 'scroll.cell_align', 'scroll.shading', 'scroll.borders', 'scroll.table_align',
  'scroll.page_break', 'scroll.section_break', 'scroll.page_setup', 'scroll.header_footer', 'scroll.page_number',
  'scroll.page_numbering', 'scroll.first_page', 'scroll.insert_picture', 'scroll.picture_size', 'scroll.link', 'scroll.unlink',
]

const allButtons = (ui) => [...ui.tabs, ...ui.panels].flatMap((t) => t.groups.flatMap((g) => g.buttons))

describe('Scroll ribbon', () => {
  it('keeps its tabs and fills the writing ones', () => {
    const ui = editorUi('scroll', COMMANDS)
    expect(ui.tabs.map((t) => t.name)).toEqual(['Home', 'Insert', 'Layout', 'Review', 'View'])
    for (const name of ['Home', 'Insert', 'Layout']) expect(ui.tabs.find((t) => t.name === name).groups.length).toBeGreaterThan(2)
    expect(ui.tabs[0].groups.map((g) => g.label)).toEqual(['Clipboard', 'Font', 'Typeface', 'Paragraph', 'Styles', 'Editing'])
    expect(ui.panels.map((p) => p.id)).toEqual(['text', 'paragraph', 'styles', 'insert', 'table', 'page'])
  })

  it('names only commands the document knows, with icons it can draw', () => {
    const ui = editorUi('scroll', COMMANDS)
    for (const btn of allButtons(ui)) {
      expect(btn.ready, btn.title).toBe(true)
      if (btn.command) expect(COMMANDS, btn.title).toContain(btn.command)
      if (btn.intent) expect([...INTENTS, ...Object.keys(INSERT_INTENTS)], btn.title).toContain(btn.intent)
      expect(btn.icon.startsWith('<') || ICON_NAMES.includes(btn.icon), btn.title).toBe(true)
    }
    const bold = allButtons(ui).find((b) => b.title.startsWith('Bold'))
    expect(bold).toMatchObject({ command: 'scroll.toggle', args: { prop: 'bold' }, stateKey: 'bold' })
  })

  it('answers the key intents with its own commands', () => {
    for (const name of Object.keys(scroll.intents)) {
      expect(INTENTS).toContain(name)
      expect(COMMANDS).toContain(scroll.intents[name].command)
    }
    expect(intentFor('scroll', 'edit.insert_text')).toEqual({ command: 'scroll.insert_text', args: {} })
    expect(intentFor('scroll', 'edit.backspace')).toEqual({ command: 'scroll.delete', args: { back: true } })
    expect(intentFor('scroll', 'format.underline')).toEqual({ command: 'scroll.toggle', args: { prop: 'underline' } })
    // the caret moves through the editing ground, not a command
    expect(intentFor('scroll', 'caret.left')).toBe(null)
  })

  it('turns a picked picture and a typed address into commands', async () => {
    expect(toBase64(new Uint8Array([104, 105, 33]))).toBe('aGkh')
    expect(toBase64(new Uint8Array([1, 2]))).toBe('AQI=')
    const file = { name: 'logo.png', arrayBuffer: async () => new Uint8Array([137, 80, 78, 71]).buffer }
    expect(await pictureCommand(file, { width: 72 })).toEqual({ command: 'scroll.insert_picture', args: { data: 'iVBORw==', name: 'logo.png', width: 72 } })
    expect(linkCommand('example.com')).toEqual({ command: 'scroll.link', args: { url: 'https://example.com' } })
    expect(linkCommand('a@b.org', 'mail me')).toEqual({ command: 'scroll.link', args: { url: 'mailto:a@b.org', text: 'mail me' } })
    expect(linkCommand('#top')).toEqual({ command: 'scroll.link', args: { url: '#top' } })
    expect(linkCommand('  ')).toBe(null)
    for (const v of Object.values(INSERT_INTENTS)) expect(COMMANDS).toContain(v.command)
  })
})
