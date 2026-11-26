// The room shell's pure parts: the editors, the ribbon's tabs, the history read for the AI's changes,
// and the shortcuts. Nothing here knows any host: each host passes its own api in.

export const EDITORS = [
  { id: 'scroll', label: 'Scroll', what: 'Documents', ext: 'docx' },
  { id: 'ledger', label: 'Ledger', what: 'Sheets', ext: 'xlsx' },
  { id: 'stage', label: 'Stage', what: 'Slides', ext: 'pptx' },
  { id: 'folio', label: 'Folio', what: 'PDFs', ext: 'pdf' },
]

export const LAYOUTS = ['classic', 'studio']

// Every editor opens a blank page until files open (the next milestone).
export const EARLY_BADGE = 'Early - opening files first'

// The ribbon's tab names, per editor; their buttons are placeholders until each editor lands.
export const RIBBON_TABS = {
  scroll: ['Home', 'Insert', 'Layout', 'Review', 'View'],
  ledger: ['Home', 'Insert', 'Formulas', 'Data', 'View'],
  stage: ['Home', 'Insert', 'Design', 'Transitions', 'Slide Show'],
  folio: ['Home', 'Annotate', 'Pages', 'Forms', 'View'],
}

/** The editor by id; Scroll for anything unknown. */
export function editorOf(id) {
  return EDITORS.find((e) => e.id === id) ?? EDITORS[0]
}

/** 'classic' or 'studio'; classic for anything else. */
export function layoutOf(value) {
  return LAYOUTS.includes(value) ? value : 'classic'
}

/** The title bar's words: "Scriptorium · Scroll". */
export function titleFor(editor) {
  return `Scriptorium · ${editorOf(editor).label}`
}

/** The ribbon's tabs for an editor. */
export function ribbonTabs(editor) {
  return RIBBON_TABS[editorOf(editor).id]
}

/** JSON text (or an object already) as a value; null for anything unreadable. */
export function parseJson(value) {
  if (value === null || value === undefined) return null
  if (typeof value !== 'string') return value
  try {
    return JSON.parse(value)
  } catch {
    return null
  }
}

/** A display list from the host: an object with a size, or null. */
export function readList(value) {
  const list = parseJson(value)
  return list && typeof list === 'object' && Number(list.w) > 0 && Number(list.h) > 0 && Array.isArray(list.items) ? list : null
}

/** The bus's history as a list of steps (oldest first, the undone ones after). */
export function readHistory(value) {
  const list = parseJson(value)
  if (!Array.isArray(list)) return []
  return list
    .filter((s) => s && typeof s === 'object' && Number.isFinite(Number(s.id)))
    .map((s) => ({ id: Number(s.id), origin: s.origin === 'ai' ? 'ai' : 'user', label: String(s.label ?? ''), undone: s.undone === true }))
}

/** What the Undo and Redo buttons can do now. */
export function undoState(history) {
  const steps = history ?? []
  return { canUndo: steps.some((s) => !s.undone), canRedo: steps.some((s) => s.undone) }
}

/** The AI's changes still on the page, newest first, each with its undo. Only the newest step of all can
 * be undone (the bus undoes in order), so an older AI step says why it waits. */
export function aiPills(history, max = 6) {
  const done = (history ?? []).filter((s) => !s.undone)
  const newest = done.at(-1)?.id
  return done
    .filter((s) => s.origin === 'ai')
    .reverse()
    .slice(0, max)
    .map((s) => ({ id: s.id, label: s.label || 'a change', undoable: s.id === newest }))
}

/** The answer of a command, undo or redo: { ok, step, pages } or { ok: false, error }. */
export function readDone(value) {
  const done = parseJson(value)
  if (!done || typeof done !== 'object') return { ok: false, error: 'No answer.' }
  return done
}

/** Plain words for a failure (a rejected promise from the host is often a bare string). */
export function plainError(reason) {
  if (!reason) return ''
  if (typeof reason === 'string') return reason
  return String(reason.message ?? reason.error ?? reason)
}

/** True when a key press belongs to a field the member is typing in (the chat keeps its own undo). */
export function isTypingTarget(el) {
  if (!el || typeof el !== 'object') return false
  const tag = String(el.tagName ?? '').toLowerCase()
  return tag === 'input' || tag === 'textarea' || tag === 'select' || el.isContentEditable === true
}

/** 'undo', 'redo' or null for a key press (Ctrl/Cmd+Z, Ctrl/Cmd+Shift+Z, Ctrl/Cmd+Y). */
export function shortcutOf(e) {
  if (!e || !(e.ctrlKey || e.metaKey) || e.altKey) return null
  const key = String(e.key ?? '').toLowerCase()
  if (key === 'z') return e.shiftKey ? 'redo' : 'undo'
  if (key === 'y' && !e.shiftKey) return 'redo'
  return null
}

/** A recent file's editor and short name, for the Studio list. */
export function recentRows(list) {
  return (Array.isArray(list) ? list : [])
    .filter((f) => f && typeof f.path === 'string' && f.path)
    .map((f) => ({ path: f.path, name: f.name || f.path.split(/[\\/]/).pop(), editor: editorOf(f.editor).id }))
}

/** A file name to offer in the save picker for an editor's document. */
export function saveName(editor, name = 'Untitled') {
  return `${String(name || 'Untitled').replace(/[\\/:*?"<>|]+/g, ' ').trim() || 'Untitled'}.${editorOf(editor).ext}`
}
