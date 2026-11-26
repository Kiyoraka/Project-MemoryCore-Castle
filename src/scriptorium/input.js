// The page's keys and typing as editor-neutral intents: { command, args } where `command` is a neutral
// name (caret.left, edit.backspace, format.bold, clip.copy, ...). The room moves the caret itself
// (caret.*, select.all) and runs the rest through the editor's own `intents` map
// (editors/<editor>/index.js), which turns them into that editor's bus commands. Nothing here knows any
// host or any editor.
//
//   caret.left / right / up / down        args { extend }   Shift extends the selection
//   caret.up / down                       args { ctrl: true, extend }  Ctrl+Up / Ctrl+Down (a sheet's
//                                                           data edge; the ground's plain up/down elsewhere)
//   caret.word_left / word_right          args { extend }   Ctrl+Left / Ctrl+Right
//   caret.line_start / line_end           args { extend }   Home / End
//   caret.doc_start / doc_end             args { extend }   Ctrl+Home / Ctrl+End
//   select.all                                              Ctrl+A
//   edit.insert_text                      args { text }     typing, and an IME's finished word
//   edit.backspace / edit.delete          args { word }     Backspace / Delete (Ctrl: a whole word)
//   edit.enter                            args { shift, ctrl }  Enter (Shift+Enter, Ctrl+Enter)
//   edit.tab                              args { back }     Tab / Shift+Tab
//   format.bold / italic / underline                        Ctrl+B / I / U
//   clip.copy / clip.cut                                    Ctrl+C / Ctrl+X
//   clip.paste                            args { text, html? }  Ctrl+V (the text arrives with the paste event)
//   history.undo / history.redo                             Ctrl+Z / Ctrl+Y (Ctrl+Shift+Z)
//   find.open / find.replace                                Ctrl+F / Ctrl+H
//   file.print / file.save / file.save_as / file.open       Ctrl+P / Ctrl+S / Ctrl+Shift+S / Ctrl+O

export const INTENTS = [
  'caret.left',
  'caret.right',
  'caret.up',
  'caret.down',
  'caret.word_left',
  'caret.word_right',
  'caret.line_start',
  'caret.line_end',
  'caret.doc_start',
  'caret.doc_end',
  'select.all',
  'edit.insert_text',
  'edit.backspace',
  'edit.delete',
  'edit.enter',
  'edit.tab',
  'format.bold',
  'format.italic',
  'format.underline',
  'clip.copy',
  'clip.cut',
  'clip.paste',
  'history.undo',
  'history.redo',
  'find.open',
  'find.replace',
  'file.print',
  'file.save',
  'file.save_as',
  'file.open',
]

// key presses whose work the browser must still do: Ctrl+V's paste event carries the clipboard's text
export const NATIVE_INTENTS = new Set(['clip.paste'])

// the hidden typing field carries this attribute, so the room knows it is the page and not a text box
export const PAGE_INPUT_ATTR = 'data-scr-input'

const intent = (command, args = {}) => ({ command, args })

const ARROWS = { ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down' }

const CTRL_LETTERS = {
  a: () => intent('select.all'),
  b: () => intent('format.bold'),
  i: () => intent('format.italic'),
  u: () => intent('format.underline'),
  c: () => intent('clip.copy'),
  x: () => intent('clip.cut'),
  v: () => intent('clip.paste'),
  y: (shift) => (shift ? null : intent('history.redo')),
  z: (shift) => intent(shift ? 'history.redo' : 'history.undo'),
  f: () => intent('find.open'),
  h: () => intent('find.replace'),
  p: () => intent('file.print'),
  s: (shift) => intent(shift ? 'file.save_as' : 'file.save'),
  o: (shift) => (shift ? null : intent('file.open')),
}

/** The intent of a key press (a KeyboardEvent or anything shaped like one), or null for a key that is
 * typing (it arrives as text) or means nothing on the page. */
export function keyIntent(e) {
  if (!e || e.isComposing || e.key === 'Process' || e.keyCode === 229) return null
  const key = String(e.key ?? '')
  const mod = !!(e.ctrlKey || e.metaKey)
  const shift = !!e.shiftKey
  // Alt (and AltGr, which comes as Ctrl+Alt) types characters on many keyboards
  if (e.altKey) return null
  if (mod) {
    const letter = key.length === 1 ? key.toLowerCase() : ''
    if (letter && CTRL_LETTERS[letter]) return CTRL_LETTERS[letter](shift)
    if (key === 'ArrowLeft') return intent('caret.word_left', { extend: shift })
    if (key === 'ArrowRight') return intent('caret.word_right', { extend: shift })
    if (key === 'ArrowUp') return intent('caret.up', { ctrl: true, extend: shift })
    if (key === 'ArrowDown') return intent('caret.down', { ctrl: true, extend: shift })
    if (key === 'Home') return intent('caret.doc_start', { extend: shift })
    if (key === 'End') return intent('caret.doc_end', { extend: shift })
    if (key === 'Backspace') return intent('edit.backspace', { word: true })
    if (key === 'Delete') return intent('edit.delete', { word: true })
    if (key === 'Enter') return intent('edit.enter', { shift, ctrl: true })
    return null
  }
  if (ARROWS[key]) return intent(`caret.${ARROWS[key]}`, { extend: shift })
  if (key === 'Home') return intent('caret.line_start', { extend: shift })
  if (key === 'End') return intent('caret.line_end', { extend: shift })
  if (key === 'Backspace') return intent('edit.backspace', { word: false })
  if (key === 'Delete') return intent('edit.delete', { word: false })
  if (key === 'Enter') return intent('edit.enter', { shift, ctrl: false })
  if (key === 'Tab') return intent('edit.tab', { back: shift })
  return null
}

/** True for the page's hidden typing field. */
export function isPageInput(el) {
  return !!el && typeof el.hasAttribute === 'function' && el.hasAttribute(PAGE_INPUT_ATTR)
}

/** Wires a (hidden) textarea as the page's keyboard: key presses become intents, typed text and an
 * IME's finished composition become edit.insert_text, a paste becomes clip.paste with its text, and the
 * browser's own copy and cut become clip.copy and clip.cut. `onIntent({ command, args })` hears them
 * all. Answers a function that unwires it. */
export function attachTypist(el, onIntent) {
  let composing = false
  const take = () => {
    const text = String(el.value ?? '')
    el.value = ''
    return text
  }
  const on = {
    keydown(e) {
      if (composing || e.isComposing) return
      const it = keyIntent(e)
      if (!it) return
      if (NATIVE_INTENTS.has(it.command)) return
      e.preventDefault()
      e.stopPropagation?.()
      onIntent(it)
    },
    compositionstart() {
      composing = true
    },
    compositionend(e) {
      composing = false
      const typed = take()
      const text = e && typeof e.data === 'string' && e.data ? e.data : typed
      if (text) onIntent(intent('edit.insert_text', { text }))
    },
    input(e) {
      if (composing || e?.isComposing) return
      const text = take()
      if (text) onIntent(intent('edit.insert_text', { text }))
    },
    paste(e) {
      e.preventDefault()
      const data = e.clipboardData
      const text = data ? String(data.getData('text/plain') ?? '') : ''
      const html = data ? String(data.getData('text/html') ?? '') : ''
      onIntent(intent('clip.paste', html ? { text, html } : { text }))
    },
    copy(e) {
      e.preventDefault()
      onIntent(intent('clip.copy'))
    },
    cut(e) {
      e.preventDefault()
      onIntent(intent('clip.cut'))
    },
  }
  for (const [name, fn] of Object.entries(on)) el.addEventListener(name, fn)
  return () => {
    for (const [name, fn] of Object.entries(on)) el.removeEventListener(name, fn)
  }
}
