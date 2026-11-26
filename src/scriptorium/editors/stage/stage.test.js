import { describe, expect, it } from 'vitest'
import stage, { actions, intents, panels, tabs } from './index.js'
import { letterbox, showClick, showKey, showList, startAt } from './show.js'
import { editorUi, intentFor } from '../../ribbon.js'

// The commands the Rust side lists (stage::cmds::specs and the open-and-see ones).
const COMMANDS = new Set([
  'stage.set_shape_text', 'stage.set_notes', 'stage.insert_text', 'stage.delete', 'stage.enter', 'stage.tab', 'stage.select_all', 'stage.select_slide',
  'stage.arrow', 'stage.paste', 'stage.format', 'stage.bold', 'stage.italic', 'stage.underline', 'stage.strike', 'stage.grow_font', 'stage.shrink_font',
  'stage.align', 'stage.line_spacing', 'stage.bullets', 'stage.numbering', 'stage.list_level', 'stage.autofit', 'stage.wrap', 'stage.insert_text_box',
  'stage.insert_shape', 'stage.insert_line', 'stage.insert_picture', 'stage.insert_table', 'stage.table_insert', 'stage.table_delete', 'stage.table_size',
  'stage.cell_fill', 'stage.fill', 'stage.outline', 'stage.crop', 'stage.order', 'stage.group', 'stage.ungroup', 'stage.regroup', 'stage.align_shapes',
  'stage.rotate', 'stage.flip', 'stage.size', 'stage.position', 'stage.nudge', 'stage.duplicate', 'stage.delete_shapes', 'stage.new_slide',
  'stage.duplicate_slide', 'stage.delete_slide', 'stage.move_slide', 'stage.set_layout', 'stage.restore_parts',
])

const allButtons = () => [...tabs.flatMap((t) => t.groups), ...panels.flatMap((p) => p.groups)].flatMap((g) => g.buttons)

describe('Stage on the page', () => {
  it('keeps the ribbon tabs the room knows, with real buttons', () => {
    expect(tabs.map((t) => t.name)).toEqual(['Home', 'Insert', 'Design', 'Transitions', 'Slide Show'])
    for (const b of allButtons()) {
      expect(b.command || b.intent, JSON.stringify(b)).toBeTruthy()
      if (b.command) expect(COMMANDS.has(b.command), b.command).toBe(true)
      expect(typeof b.title).toBe('string')
    }
    const ui = editorUi('stage', [...COMMANDS])
    expect(ui.panels.map((p) => p.id)).toEqual(['shape', 'size', 'text', 'table'])
    expect(ui.tabs[0].groups.every((g) => g.buttons.every((b) => b.ready))).toBe(true)
    expect(stage.actions).toBe(actions)
  })

  it('answers the keys as its own commands', () => {
    for (const v of Object.values(intents)) expect(COMMANDS.has(v.command), v.command).toBe(true)
    expect(intentFor('stage', 'edit.backspace')).toEqual({ command: 'stage.delete', args: { back: true } })
    expect(intentFor('stage', 'caret.left')).toEqual({ command: 'stage.arrow', args: { dir: 'left' } })
    expect(intentFor('stage', 'caret.word_left')).toBe(null)
    expect(typeof actions['stage.show']).toBe('function')
  })

  it('leaves the prompt text out of the show', () => {
    const list = { w: 720, h: 540, items: [{ t: 'glyphs', c: '#808080fe', g: [] }, { t: 'glyphs', c: '#000000', g: [] }, { t: 'path', d: 'M0 0' }] }
    expect(showList(list).items.length).toBe(2)
    expect(showList(JSON.stringify(list)).items[0].c).toBe('#000000')
    expect(showList('nope')).toBe(null)
  })

  it('moves through the show by keys and clicks', () => {
    expect(showKey('ArrowRight', 0, 3)).toEqual({ to: 1 })
    expect(showKey(' ', 2, 3)).toEqual({ to: 3 }) // the black end screen
    expect(showKey(' ', 3, 3)).toEqual({ end: true })
    expect(showKey('PageUp', 0, 3)).toEqual({ to: 0 })
    expect(showKey('ArrowLeft', 3, 3)).toEqual({ to: 2 })
    expect(showKey('Escape', 1, 3)).toEqual({ end: true })
    expect(showKey('2', 0, 3)).toEqual({ digit: '2' })
    expect(showKey('Enter', 0, 3, '2')).toEqual({ to: 1 })
    expect(showKey('Enter', 0, 3, '9')).toEqual({ to: 2 })
    expect(showKey('x', 0, 3)).toBe(null)
    expect(showClick(0, 0, 2)).toEqual({ to: 1 })
    expect(showClick(2, 1, 2)).toEqual({ to: 0 })
    expect(showClick(0, 2, 2)).toEqual({ end: true })
    expect(startAt('start', 4, 9)).toBe(0)
    expect(startAt('current', 4, 9)).toBe(4)
    expect(startAt('current', 12, 9)).toBe(8)
  })

  it('letterboxes a slide in the screen', () => {
    expect(letterbox(960, 540, 1920, 1200)).toEqual({ x: 0, y: 60, scale: 2 })
    expect(letterbox(720, 540, 1600, 900).x).toBeCloseTo(200)
    expect(letterbox(0, 0, 10, 10)).toEqual({ x: 0, y: 0, scale: 1 })
  })
})
