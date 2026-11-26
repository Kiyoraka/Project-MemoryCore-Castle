import { describe, expect, it } from 'vitest'
import folio, { bookmarkCommand, fitZoom, intents, outlineRows, panels, tabs } from './index.js'
import { editorUi, intentFor } from '../../ribbon.js'

describe("Folio's ribbon", () => {
  it('has the tier-1 tabs and every button names a folio command or a neutral intent', () => {
    expect(tabs.map((t) => t.name)).toEqual(['Home', 'Annotate', 'Pages', 'Forms', 'View'])
    const all = [...tabs, ...panels].flatMap((t) => t.groups.flatMap((g) => g.buttons))
    for (const b of all) {
      expect(typeof b.title).toBe('string')
      expect(!!b.command !== !!b.intent, b.title).toBe(true)
      if (b.command) expect(b.command.startsWith('folio.'), b.title).toBe(true)
    }
    expect(folio.tabs).toBe(tabs)
  })

  it('reads safely through the registry, the buttons ready once the document lists their commands', () => {
    const ui = editorUi('folio', ['folio.go_to_page', 'folio.delete_pages'])
    const view = ui.tabs.find((t) => t.name === 'View')
    const first = view.groups[0].buttons[0]
    expect(first).toMatchObject({ command: 'folio.go_to_page', args: { to: 'first' }, ready: true })
    const organise = ui.tabs.find((t) => t.name === 'Pages').groups[0].buttons
    expect(organise.find((b) => b.args.angle === 90).ready).toBe(false)
    expect(ui.panels.map((p) => p.id)).toEqual(['bookmarks', 'fields', 'pages'])
  })

  it('answers typing, Tab and Enter with its own commands (caret moves stay the ground’s)', () => {
    expect(intentFor('folio', 'edit.insert_text')).toEqual({ command: 'folio.insert_text', args: {} })
    expect(intentFor('folio', 'edit.tab')).toEqual({ command: 'folio.next_field', args: {} })
    expect(intents['edit.enter'].args.text).toBe('\n')
    expect(intentFor('folio', 'caret.left')).toBeNull()
    expect(intentFor('folio', 'select.all')).toBeNull()
  })
})

describe('fit page and fit width', () => {
  it('fits the whole page or its width inside the padding', () => {
    expect(fitZoom(612, 792, 644, 1000, 'width')).toBe(1)
    expect(fitZoom(612, 792, 644, 432, 'page')).toBe(0.505)
    expect(fitZoom(612, 792, 1000, 824)).toBe(1)
    expect(fitZoom(0, 792, 100, 100)).toBe(1)
    expect(fitZoom('x', 1, 1, 1)).toBe(1)
  })
})

describe('the bookmarks list', () => {
  const tree = {
    outline: [
      { index: 0, title: 'One', page: 0, children: [{ index: 1, title: 'One a', page: 1, children: [] }] },
      { index: 2, title: 'Nowhere', page: null, children: [] },
    ],
  }
  it('flattens the tree with depths and goes by index', () => {
    const rows = outlineRows(tree)
    expect(rows).toEqual([
      { index: 0, title: 'One', page: 0, depth: 0 },
      { index: 1, title: 'One a', page: 1, depth: 1 },
      { index: 2, title: 'Nowhere', page: null, depth: 0 },
    ])
    expect(bookmarkCommand(rows[1])).toEqual({ command: 'folio.bookmark', args: { index: 1 } })
    expect(bookmarkCommand(rows[2])).toBeNull()
    expect(outlineRows(null)).toEqual([])
    expect(outlineRows([{ title: 7, children: 'x' }])).toEqual([{ index: 0, title: '7', page: null, depth: 0 }])
  })
})
