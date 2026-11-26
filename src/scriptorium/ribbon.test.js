import { describe, expect, it } from 'vitest'
import {
  FIND_SOON,
  OVERLAY_INTENTS,
  REGISTRY,
  VIEW_PLACES,
  actionFor,
  askingFor,
  buttonCommand,
  commandNames,
  editorUi,
  editorViews,
  findSupport,
  intentFor,
  normalButton,
  notYet,
  overlayFor,
  pressed,
  ribbonTabs,
  tabGroups,
  viewProps,
  viewToggle,
} from './ribbon.js'
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
  it("finds a tab\'s groups, falling back to the first tab", () => {
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
  it("gives a button\'s command the selection", () => {
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
  it("answers an editor\'s intents and says what comes later", () => {
    // every editor types now: its own command for the neutral intent
    expect(intentFor('scroll', 'edit.insert_text')).toEqual({ command: 'scroll.insert_text', args: {} })
    expect(intentFor('ledger', 'edit.insert_text')?.command).toBe('ledger.insert_text')
    expect(intentFor('stage', 'edit.insert_text')?.command).toBe('stage.insert_text')
    expect(intentFor('folio', 'edit.insert_text')?.command).toBe('folio.insert_text')
    expect(intentFor('scroll', 'no.such_intent')).toBe(null)
    REGISTRY.scroll.intents['test.only'] = { command: 'scroll.test_only' }
    try {
      expect(intentFor('scroll', 'test.only')).toEqual({ command: 'scroll.test_only', args: {} })
    } finally {
      delete REGISTRY.scroll.intents['test.only']
    }
    expect(notYet('ledger', 'edit.insert_text')).toBe('Typing comes to Ledger soon.')
    expect(notYet('stage', 'format.bold')).toBe('Formatting comes to Stage soon.')
    expect(notYet('folio', 'clip.paste')).toBe('Paste and cut come to Folio soon.')
    expect(notYet('scroll', 'other')).toBe('That comes to Scroll soon.')
  })
  it("draws a named icon or an editor\'s own markup", () => {
    expect(iconOf('print', 20)).toContain('width="20"')
    expect(iconOf('<path d="M1 1h2"/>')).toContain('<path d="M1 1h2"/>')
    expect(iconOf('no-such-icon')).toBe('')
  })
  it("finds an editor's own page-side actions", () => {
    expect(typeof actionFor('stage', 'stage.show')).toBe('function')
    expect(typeof actionFor('stage', 'stage.pick_picture')).toBe('function')
    expect(actionFor('stage', 'stage.nothing')).toBe(null)
    expect(actionFor('scroll', 'stage.show')).toBe(null)
    expect(actionFor('stage', '')).toBe(null)
    expect(actionFor('stage', null)).toBe(null)
  })
  it("places an editor's views and gives them their props", () => {
    const stage = editorViews('stage')
    expect(stage.map((v) => [v.name, v.place])).toEqual([
      ['slideShow', 'overlay'],
      ['notes', 'below'],
    ])
    expect(stage.every((v) => typeof v.load === 'function')).toBe(true)
    expect(editorViews('folio').map((v) => [v.name, v.place])).toEqual([['bookmarks', 'side']])
    expect(editorViews('scroll')).toEqual([])
    expect(VIEW_PLACES.slideShow).toBe('overlay')
    expect(viewToggle('stage', 'stage.notes')).toBe('notes')
    expect(viewToggle('stage', 'stage.slideShow')).toBe(null)
    expect(viewToggle('stage', 'stage.show')).toBe(null)
    expect(viewToggle('scroll', 'stage.notes')).toBe(null)
    expect(viewToggle('folio', 'folio.bookmarks')).toBe('bookmarks')
    const api = {}
    expect(viewProps('notes', { api, doc: 3, page: 2 })).toEqual({ api, doc: 3, slide: 2 })
    expect(viewProps('bookmarks', { outline: [{ title: 'A' }], page: 1, api })).toEqual({ outline: [{ title: 'A' }], page: 1 })
    expect(viewProps('bookmarks', {})).toEqual({ outline: [], page: -1 })
    expect(viewProps('slideShow', { api, doc: 1, start: 4, count: 0 })).toEqual({ api, doc: 1, start: 4, count: 1 })
    expect(viewProps('other', { api, doc: 1, page: 5 })).toEqual({ api, doc: 1, page: 5 })
  })
  it("opens the slide show in the room's overlay at the right slide", () => {
    expect(OVERLAY_INTENTS['stage.show']).toBe('slideShow')
    expect(overlayFor('stage', 'stage.show', { from: 'start' }, 3, 5)).toMatchObject({ name: 'slideShow', start: 0 })
    expect(overlayFor('stage', 'stage.show', { from: 'current' }, 3, 5).start).toBe(3)
    expect(overlayFor('stage', 'stage.show', {}, 9, 5).start).toBe(4)
    expect(typeof overlayFor('stage', 'stage.show').load).toBe('function')
    expect(overlayFor('scroll', 'stage.show')).toBe(null)
    expect(overlayFor('stage', 'stage.notes')).toBe(null)
  })
  it("asks for a picture file or a link's address before Scroll's insert commands", async () => {
    const pic = askingFor('scroll', 'insert.picture')
    expect(pic).toMatchObject({ command: 'scroll.insert_picture', asks: 'file' })
    expect(pic.accept).toContain('image/png')
    const file = { name: 'dot.png', arrayBuffer: async () => new Uint8Array([1, 2, 3]).buffer }
    expect(await pic.build(file)).toEqual({ command: 'scroll.insert_picture', args: { data: 'AQID', name: 'dot.png' } })
    const link = askingFor('scroll', 'insert.link')
    expect(link).toMatchObject({ command: 'scroll.link', asks: 'url' })
    expect(link.build('example.com')).toEqual({ command: 'scroll.link', args: { url: 'https://example.com' } })
    expect(link.build('  ')).toBe(null)
    expect(askingFor('stage', 'insert.link')).toBe(null)
    expect(askingFor('scroll', 'insert.table')).toBe(null)
  })
})
