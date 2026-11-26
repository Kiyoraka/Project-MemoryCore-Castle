import { describe, expect, it, vi } from 'vitest'
import { INTENTS, NATIVE_INTENTS, PAGE_INPUT_ATTR, attachTypist, isPageInput, keyIntent } from './input.js'

const key = (k, mods = {}) => ({ key: k, ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, ...mods })

describe('the key map', () => {
  it('moves the caret, Shift extending', () => {
    expect(keyIntent(key('ArrowLeft'))).toEqual({ command: 'caret.left', args: { extend: false } })
    expect(keyIntent(key('ArrowRight', { shiftKey: true }))).toEqual({ command: 'caret.right', args: { extend: true } })
    expect(keyIntent(key('ArrowUp')).command).toBe('caret.up')
    expect(keyIntent(key('ArrowDown', { shiftKey: true }))).toEqual({ command: 'caret.down', args: { extend: true } })
    expect(keyIntent(key('Home'))).toEqual({ command: 'caret.line_start', args: { extend: false } })
    expect(keyIntent(key('End', { shiftKey: true }))).toEqual({ command: 'caret.line_end', args: { extend: true } })
    expect(keyIntent(key('Home', { ctrlKey: true }))).toEqual({ command: 'caret.doc_start', args: { extend: false } })
    expect(keyIntent(key('End', { metaKey: true, shiftKey: true }))).toEqual({ command: 'caret.doc_end', args: { extend: true } })
    expect(keyIntent(key('ArrowLeft', { ctrlKey: true })).command).toBe('caret.word_left')
    expect(keyIntent(key('ArrowRight', { ctrlKey: true, shiftKey: true }))).toEqual({ command: 'caret.word_right', args: { extend: true } })
    // Ctrl+Up / Ctrl+Down: up and down with ctrl (Ledger jumps to the data edge)
    expect(keyIntent(key('ArrowUp', { ctrlKey: true }))).toEqual({ command: 'caret.up', args: { ctrl: true, extend: false } })
    expect(keyIntent(key('ArrowDown', { ctrlKey: true, shiftKey: true }))).toEqual({ command: 'caret.down', args: { ctrl: true, extend: true } })
    expect(keyIntent(key('ArrowDown', { metaKey: true })).args.ctrl).toBe(true)
  })
  it('edits', () => {
    expect(keyIntent(key('Backspace'))).toEqual({ command: 'edit.backspace', args: { word: false } })
    expect(keyIntent(key('Backspace', { ctrlKey: true }))).toEqual({ command: 'edit.backspace', args: { word: true } })
    expect(keyIntent(key('Delete'))).toEqual({ command: 'edit.delete', args: { word: false } })
    expect(keyIntent(key('Delete', { ctrlKey: true })).args.word).toBe(true)
    expect(keyIntent(key('Enter'))).toEqual({ command: 'edit.enter', args: { shift: false, ctrl: false } })
    expect(keyIntent(key('Enter', { shiftKey: true })).args).toEqual({ shift: true, ctrl: false })
    expect(keyIntent(key('Enter', { ctrlKey: true })).args).toEqual({ shift: false, ctrl: true })
    expect(keyIntent(key('Tab'))).toEqual({ command: 'edit.tab', args: { back: false } })
    expect(keyIntent(key('Tab', { shiftKey: true }))).toEqual({ command: 'edit.tab', args: { back: true } })
  })
  it('knows the Ctrl letters', () => {
    const ctrl = (k, shift = false) => keyIntent(key(k, { ctrlKey: true, shiftKey: shift }))?.command ?? null
    expect(ctrl('b')).toBe('format.bold')
    expect(ctrl('I')).toBe('format.italic')
    expect(ctrl('u')).toBe('format.underline')
    expect(ctrl('a')).toBe('select.all')
    expect(ctrl('c')).toBe('clip.copy')
    expect(ctrl('x')).toBe('clip.cut')
    expect(ctrl('v')).toBe('clip.paste')
    expect(ctrl('z')).toBe('history.undo')
    expect(ctrl('z', true)).toBe('history.redo')
    expect(ctrl('y')).toBe('history.redo')
    expect(ctrl('y', true)).toBe(null)
    expect(ctrl('f')).toBe('find.open')
    expect(ctrl('h')).toBe('find.replace')
    expect(ctrl('p')).toBe('file.print')
    expect(ctrl('s')).toBe('file.save')
    expect(ctrl('s', true)).toBe('file.save_as')
    expect(ctrl('o')).toBe('file.open')
    expect(ctrl('q')).toBe(null)
    expect(keyIntent(key('b', { metaKey: true })).command).toBe('format.bold')
  })
  it('leaves typing, IME and Alt keys alone', () => {
    expect(keyIntent(key('a'))).toBe(null)
    expect(keyIntent(key(' '))).toBe(null)
    expect(keyIntent(key('PageDown'))).toBe(null)
    expect(keyIntent(key('ArrowLeft', { isComposing: true }))).toBe(null)
    expect(keyIntent(key('Process'))).toBe(null)
    expect(keyIntent({ ...key('Enter'), keyCode: 229 })).toBe(null)
    expect(keyIntent(key('e', { ctrlKey: true, altKey: true }))).toBe(null)
    expect(keyIntent(null)).toBe(null)
  })
  it('names every intent it can make', () => {
    const made = new Set()
    for (const k of ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', 'Home', 'End', 'Backspace', 'Delete', 'Enter', 'Tab', ...'abcdefghijklmnopqrstuvwxyz'])
      for (const ctrlKey of [false, true]) for (const shiftKey of [false, true]) {
        const it = keyIntent(key(k, { ctrlKey, shiftKey }))
        if (it) made.add(it.command)
      }
    for (const c of made) expect(INTENTS).toContain(c)
    expect(INTENTS).toContain('edit.insert_text')
    expect(NATIVE_INTENTS.has('clip.paste')).toBe(true)
  })
})

function fakeField() {
  const handlers = {}
  return {
    value: '',
    attrs: { [PAGE_INPUT_ATTR]: '' },
    hasAttribute(name) {
      return name in this.attrs
    },
    addEventListener: (name, fn) => (handlers[name] = fn),
    removeEventListener: (name, fn) => handlers[name] === fn && delete handlers[name],
    fire(name, e = {}) {
      handlers[name]?.({ preventDefault: vi.fn(), stopPropagation: vi.fn(), ...e })
    },
    handlers,
  }
}

describe('the typist', () => {
  it('turns keys into intents and keeps Ctrl+V for the paste event', () => {
    const el = fakeField()
    const heard = []
    attachTypist(el, (it) => heard.push(it))
    const e = { ...key('ArrowLeft', { shiftKey: true }), preventDefault: vi.fn(), stopPropagation: vi.fn() }
    el.handlers.keydown(e)
    expect(e.preventDefault).toHaveBeenCalled()
    expect(e.stopPropagation).toHaveBeenCalled()
    const paste = { ...key('v', { ctrlKey: true }), preventDefault: vi.fn(), stopPropagation: vi.fn() }
    el.handlers.keydown(paste)
    expect(paste.preventDefault).not.toHaveBeenCalled()
    const letter = { ...key('q'), preventDefault: vi.fn(), stopPropagation: vi.fn() }
    el.handlers.keydown(letter)
    expect(letter.preventDefault).not.toHaveBeenCalled()
    expect(heard).toEqual([{ command: 'caret.left', args: { extend: true } }])
  })
  it('sends typed text, an IME word once it is finished, and pastes with their text', () => {
    const el = fakeField()
    const heard = []
    const stop = attachTypist(el, (it) => heard.push(it))
    el.value = 'a'
    el.fire('input')
    expect(el.value).toBe('')
    el.fire('compositionstart')
    el.value = 'ni'
    el.fire('input', { isComposing: true })
    el.fire('keydown', key('ArrowLeft'))
    el.fire('compositionend', { data: '你' })
    el.fire('paste', { clipboardData: { getData: (t) => (t === 'text/plain' ? 'pasted' : '') } })
    el.fire('paste', { clipboardData: { getData: (t) => (t === 'text/plain' ? 'x' : '<b>x</b>') } })
    el.fire('copy')
    el.fire('cut')
    expect(heard).toEqual([
      { command: 'edit.insert_text', args: { text: 'a' } },
      { command: 'edit.insert_text', args: { text: '你' } },
      { command: 'clip.paste', args: { text: 'pasted' } },
      { command: 'clip.paste', args: { text: 'x', html: '<b>x</b>' } },
      { command: 'clip.copy', args: {} },
      { command: 'clip.cut', args: {} },
    ])
    stop()
    expect(Object.keys(el.handlers)).toEqual([])
  })
  it('knows the page input from other fields', () => {
    expect(isPageInput(fakeField())).toBe(true)
    expect(isPageInput({ hasAttribute: () => false })).toBe(false)
    expect(isPageInput(null)).toBe(false)
  })
})
