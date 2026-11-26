import { describe, expect, it } from 'vitest'
import { FIND_SOON, REGISTRY, buttonCommand, commandNames, editorUi, findSupport, intentFor, normalButton, notYet, pressed, ribbonTabs, tabGroups } from './ribbon.js'
import { iconOf } from './icons.js'

describe('the ribbon registry', () => {
  it('has each editor with the tabs it had', () => {
    expect(Object.keys(REGISTRY)).toEqual(['scroll', 'ledger', 'stage', 'folio'])
    expect(ribbonTabs('scroll')).toEqual(['Home', 'Insert', 'Layout', 'Review', 'View'])
    expect(ribbonTabs('ledger')).toEqual(['Home', 'Insert', 'Formulas', 'Data', 'View'])
    expect(ribbonTabs('stage')).toEqual(['Home', 'Insert', 'Design', 'Transitions', 'Slide Show'])
    expect(ribbonTabs('folio')).toEqual(['Home', 'Annotate', 'Pages', 'Forms', 'View'])
    expect(ribbonTabs('word')).toEqual(ribbonTabs('scroll'))
    for (const id of Object.keys(REGISTRY)) {
      const ui = editorUi(id)
      expect(Array.isArray(ui.panels)).toBe(true)
      expect(typeof ui.intents).toBe('object')
    }
  })
  it('makes buttons safe and marks the ones the document knows', () => {
    expect(normalButton({ icon: 'bold', title: 'Bold', command: 'scroll.bold', stateKey: 'bold' })).toEqual({
      icon: 'bold',
      title: 'Bold',
      command: 'scroll.bold',
      intent: null,
      args: {},
      stateKey: 'bold',
      ready: true,
    })
    expect(normalButton({ title: 'Nothing' })).toBe(null)
    expect(normalButton({ command: 'scroll.bold' }, ['scroll.italic']).ready).toBe(false)
    expect(normalButton({ intent: 'find.open', args: [1] }, [])).toMatchObject({ intent: 'find.open', args: {}, ready: true, title: 'find.open' })
  })
  it('finds a tab\'s groups, falling back to the first tab', () => {
    const ui = { tabs: [{ name: 'Home', groups: [{ label: 'Font', buttons: [] }] }, { name: 'Insert', groups: [] }] }
    expect(tabGroups(ui, 'Insert')).toEqual([])
    expect(tabGroups(ui, 'Gone')[0].label).toBe('Font')
    expect(tabGroups(null, 'Home')).toEqual([])
  })
  it('reads pressed buttons from the state', () => {
    expect(pressed({ bold: true }, 'bold')).toBe(true)
    expect(pressed({ bold: 'yes' }, 'bold')).toBe(false)
    expect(pressed({ font: { bold: true } }, 'font.bold')).toBe(true)
    expect(pressed({}, 'font.bold')).toBe(false)
    expect(pressed({ bold: true }, null)).toBe(false)
    expect(pressed(null, 'bold')).toBe(false)
  })
  it('gives a button\'s command the selection', () => {
    const sel = { anchor: 1, focus: 2 }
    expect(buttonCommand({ command: 'scroll.bold', args: { on: true } }, sel)).toEqual({ command: 'scroll.bold', args: { on: true, sel } })
    expect(buttonCommand({ command: 'scroll.bold' })).toEqual({ command: 'scroll.bold', args: {} })
  })
  it('knows what the find bar can ask for', () => {
    const names = commandNames({ commands: [{ name: 'scroll.find' }, { name: 'scroll.set_title' }, 'scroll.replace', { nope: 1 }] })
    expect(names).toEqual(['scroll.find', 'scroll.set_title', 'scroll.replace'])
    expect(commandNames(null)).toEqual([])
    expect(findSupport('scroll', names)).toEqual({ find: true, replace: true, findCommand: 'scroll.find', replaceCommand: 'scroll.replace' })
    expect(findSupport('ledger', names)).toMatchObject({ find: false, replace: false, findCommand: 'ledger.find' })
    expect(FIND_SOON).toBe('Find comes to this editor soon')
  })
  it('answers an editor\'s intents and says what comes later', () => {
    expect(intentFor('scroll', 'edit.insert_text')).toBe(null)
    REGISTRY.scroll.intents['edit.insert_text'] = { command: 'scroll.insert_text' }
    try {
      expect(intentFor('scroll', 'edit.insert_text')).toEqual({ command: 'scroll.insert_text', args: {} })
    } finally {
      delete REGISTRY.scroll.intents['edit.insert_text']
    }
    expect(notYet('ledger', 'edit.insert_text')).toBe('Typing comes to Ledger soon.')
    expect(notYet('stage', 'format.bold')).toBe('Formatting comes to Stage soon.')
    expect(notYet('folio', 'clip.paste')).toBe('Paste and cut come to Folio soon.')
    expect(notYet('scroll', 'other')).toBe('That comes to Scroll soon.')
  })
  it('draws a named icon or an editor\'s own markup', () => {
    expect(iconOf('print', 20)).toContain('width="20"')
    expect(iconOf('<path d="M1 1h2"/>')).toContain('<path d="M1 1h2"/>')
    expect(iconOf('no-such-icon')).toBe('')
  })
})
