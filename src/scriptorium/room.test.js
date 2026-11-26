import { readFileSync, readdirSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import { EARLY_BADGE, EDITORS, aiPills, editorOf, isTypingTarget, layoutOf, parseJson, plainError, readDone, readHistory, readList, recentRows, ribbonTabs, saveName, shortcutOf, titleFor, undoState } from './room.js'

describe('the editors and the layouts', () => {
  it('has the four editors in order, each with its file type', () => {
    expect(EDITORS.map((e) => e.label)).toEqual(['Scroll', 'Ledger', 'Stage', 'Folio'])
    expect(EDITORS.map((e) => e.ext)).toEqual(['docx', 'xlsx', 'pptx', 'pdf'])
    expect(editorOf('ledger').label).toBe('Ledger')
    expect(editorOf('word').id).toBe('scroll')
    expect(EARLY_BADGE).toBe('Early - opening files first')
  })
  it('names the title bar and the ribbon by the editor', () => {
    expect(titleFor('scroll')).toBe('Scriptorium · Scroll')
    expect(titleFor('folio')).toBe('Scriptorium · Folio')
    expect(ribbonTabs('scroll')[0]).toBe('Home')
    expect(ribbonTabs('ledger')).toContain('Formulas')
    expect(ribbonTabs('nothing')).toEqual(ribbonTabs('scroll'))
  })
  it('opens classic unless studio is asked for', () => {
    expect(layoutOf('studio')).toBe('studio')
    expect(layoutOf('classic')).toBe('classic')
    expect(layoutOf('Studio')).toBe('classic')
    expect(layoutOf(undefined)).toBe('classic')
  })
  it('offers a save name with the editor\'s own type', () => {
    expect(saveName('scroll')).toBe('Untitled.docx')
    expect(saveName('stage', 'Talk: v2/final')).toBe('Talk  v2 final.pptx')
    expect(saveName('folio', '   ')).toBe('Untitled.pdf')
  })
})

describe('reading the host\'s answers', () => {
  it('reads JSON text or a value, and nothing else', () => {
    expect(parseJson('{"a":1}')).toEqual({ a: 1 })
    expect(parseJson({ a: 1 })).toEqual({ a: 1 })
    expect(parseJson('nope')).toBe(null)
    expect(parseJson(null)).toBe(null)
  })
  it('takes a display list only when it has a size and items', () => {
    expect(readList('{"w":595,"h":842,"items":[]}')).toEqual({ w: 595, h: 842, items: [] })
    expect(readList('{"w":0,"h":842,"items":[]}')).toBe(null)
    expect(readList('{"w":595,"h":842}')).toBe(null)
  })
  it('reads the history and what undo and redo can do', () => {
    const history = readHistory('[{"id":1,"origin":"user","label":"a","commands":1,"undone":false},{"id":2,"origin":"ai","label":"b","commands":2,"undone":true},{"bad":true}]')
    expect(history).toEqual([
      { id: 1, origin: 'user', label: 'a', undone: false },
      { id: 2, origin: 'ai', label: 'b', undone: true },
    ])
    expect(undoState(history)).toEqual({ canUndo: true, canRedo: true })
    expect(undoState([])).toEqual({ canUndo: false, canRedo: false })
    expect(readHistory('oops')).toEqual([])
  })
  it('reads a done answer and plain words for a failure', () => {
    expect(readDone('{"ok":true,"step":3,"pages":[0]}')).toEqual({ ok: true, step: 3, pages: [0] })
    expect(readDone('').ok).toBe(false)
    expect(plainError('nothing to undo')).toBe('nothing to undo')
    expect(plainError(new Error('gone'))).toBe('gone')
    expect(plainError(null)).toBe('')
  })
})

describe('the AI\'s changes', () => {
  const step = (id, origin, label, undone = false) => ({ id, origin, label, undone })
  it('shows the AI\'s steps newest first; only the newest step of all can be undone', () => {
    const pills = aiPills([step(1, 'ai', 'Tidy'), step(2, 'user', 'mine'), step(3, 'ai', 'Bluer heading'), step(4, 'ai', 'undone one', true)])
    expect(pills).toEqual([
      { id: 3, label: 'Bluer heading', undoable: true },
      { id: 1, label: 'Tidy', undoable: false },
    ])
  })
  it('an AI step under the member\'s newer step waits', () => {
    expect(aiPills([step(1, 'ai', 'x'), step(2, 'user', 'y')])).toEqual([{ id: 1, label: 'x', undoable: false }])
  })
  it('keeps the strip short and names a step without a label', () => {
    const many = Array.from({ length: 10 }, (_, i) => step(i + 1, 'ai', ''))
    const pills = aiPills(many)
    expect(pills).toHaveLength(6)
    expect(pills[0]).toEqual({ id: 10, label: 'a change', undoable: true })
    expect(aiPills(undefined)).toEqual([])
  })
})

describe('the shortcuts', () => {
  it('knows undo and redo, on Windows and macOS', () => {
    expect(shortcutOf({ ctrlKey: true, key: 'z' })).toBe('undo')
    expect(shortcutOf({ metaKey: true, key: 'Z', shiftKey: true })).toBe('redo')
    expect(shortcutOf({ ctrlKey: true, key: 'y' })).toBe('redo')
    expect(shortcutOf({ key: 'z' })).toBe(null)
    expect(shortcutOf({ ctrlKey: true, altKey: true, key: 'z' })).toBe(null)
  })
  it('leaves a field the member types in alone', () => {
    expect(isTypingTarget({ tagName: 'TEXTAREA' })).toBe(true)
    expect(isTypingTarget({ tagName: 'INPUT' })).toBe(true)
    expect(isTypingTarget({ tagName: 'DIV', isContentEditable: true })).toBe(true)
    expect(isTypingTarget({ tagName: 'DIV' })).toBe(false)
    expect(isTypingTarget(null)).toBe(false)
  })
})

describe('recent files', () => {
  it('lists each with a short name and its editor', () => {
    expect(recentRows([{ path: 'C:\\docs\\Report.docx', name: '', editor: 'scroll' }, { path: '' }, { path: 'D:/sheets/Budget.xlsx', name: 'Budget.xlsx', editor: 'ledger' }])).toEqual([
      { path: 'C:\\docs\\Report.docx', name: 'Report.docx', editor: 'scroll' },
      { path: 'D:/sheets/Budget.xlsx', name: 'Budget.xlsx', editor: 'ledger' },
    ])
    expect(recentRows(null)).toEqual([])
  })
})

describe('the two folders rule', () => {
  // the Alice Keep copies src/scriptorium whole: nothing in it may reach into the Castle
  const files = (dir) => readdirSync(new URL(dir, import.meta.url), { withFileTypes: true }).flatMap((d) => (d.isDirectory() ? files(`${dir}${d.name}/`) : [`${dir}${d.name}`]))
  // the room shell's own files (the painter in paint/ keeps its own tests)
  it('imports nothing from outside src/scriptorium and names no Castle call', () => {
    const home = new URL('./', import.meta.url).href
    const shell = files('./').filter((f) => /\.(vue|js)$/.test(f) && !f.endsWith('.test.js') && !f.startsWith('./paint/'))
    expect(shell).toEqual(expect.arrayContaining(['./ScriptoriumRoom.vue', './layout/ClassicLayout.vue', './layout/StudioLayout.vue']))
    for (const file of shell) {
      const text = readFileSync(new URL(file, import.meta.url), 'utf8')
      for (const m of text.matchAll(/from\s+['"]([^'"]+)['"]|import\(\s*['"]([^'"]+)['"]\s*\)/g)) {
        const from = m[1] ?? m[2]
        if (from === 'vue') continue
        expect(from.startsWith('.') && new URL(from, new URL(file, import.meta.url)).href.startsWith(home), `${file} imports ${from}`).toBe(true)
      }
      expect(text, file).not.toMatch(/@tauri-apps|invoke\(|castle-memory|TerminalPanel|Castle/)
    }
  })
})
