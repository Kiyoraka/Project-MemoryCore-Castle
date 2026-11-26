// The room shell's pure parts: the editors, the history read for the AI's changes,
// and the shortcuts. Nothing here knows any host: each host passes its own api in.
import { fitPage, fitWidth } from './paint/tiles.js'

export const EDITORS = [
  { id: 'scroll', label: 'Scroll', what: 'Documents', ext: 'docx' },
  { id: 'ledger', label: 'Ledger', what: 'Sheets', ext: 'xlsx' },
  { id: 'stage', label: 'Stage', what: 'Slides', ext: 'pptx' },
  { id: 'folio', label: 'Folio', what: 'PDFs', ext: 'pdf' },
]

export const LAYOUTS = ['classic', 'studio']

// M1: files open, show and save; editing by hand grows with the next milestone.
export const EARLY_BADGE = 'Early - open and see'

// The ribbon's tabs and buttons live with each editor (editors/<editor>/index.js, read by ribbon.js).

/** The editor by id; Scroll for anything unknown. */
export function editorOf(id) {
  return EDITORS.find((e) => e.id === id) ?? EDITORS[0]
}

/** 'classic' or 'studio'; classic for anything else. */
export function layoutOf(value) {
  return LAYOUTS.includes(value) ? value : 'classic'
}

/** The title bar's words: "Scriptorium · Scroll", with the file's name after when there is one. */
export function titleFor(editor, file = '') {
  const name = String(file ?? '').trim()
  return `Scriptorium · ${editorOf(editor).label}${name ? ` · ${name}` : ''}`
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

/** What a key press asks for: 'undo', 'redo', 'open', 'save', 'saveAs' or null (Ctrl/Cmd+Z,
 * Ctrl/Cmd+Shift+Z, Ctrl/Cmd+Y, Ctrl/Cmd+O, Ctrl/Cmd+S, Ctrl/Cmd+Shift+S). */
export function shortcutOf(e) {
  if (!e || !(e.ctrlKey || e.metaKey) || e.altKey) return null
  const key = String(e.key ?? '').toLowerCase()
  if (key === 'z') return e.shiftKey ? 'redo' : 'undo'
  if (key === 'y' && !e.shiftKey) return 'redo'
  if (key === 'o' && !e.shiftKey) return 'open'
  if (key === 's') return e.shiftKey ? 'saveAs' : 'save'
  return null
}

/** -1, +1 or 0 for a key that turns the page (PageUp / PageDown without Ctrl, Alt or Cmd). */
export function pageKeyOf(e) {
  if (!e || e.ctrlKey || e.metaKey || e.altKey) return 0
  if (e.key === 'PageUp') return -1
  if (e.key === 'PageDown') return 1
  return 0
}

/** A recent file's editor and short name, for Studio's list and Classic's menu. */
export function recentRows(list) {
  return (Array.isArray(list) ? list : [])
    .filter((f) => f && typeof f.path === 'string' && f.path)
    .map((f) => ({ path: f.path, name: f.name || f.path.split(/[\\/]/).pop(), editor: editorOf(f.editor).id }))
}

/** A file name to offer in the save picker for an editor's document. */
export function saveName(editor, name = 'Untitled') {
  return `${String(name || 'Untitled').replace(/[\\/:*?"<>|]+/g, ' ').trim() || 'Untitled'}.${editorOf(editor).ext}`
}

// The types each editor opens, each saved back as its own type (a .docm stays a .docm; a .csv or .txt
// opens into Ledger as one sheet and saves back as text, or as .xlsx by Save as).
const TYPES = { scroll: ['docx', 'docm'], ledger: ['xlsx', 'xlsm', 'csv', 'txt'], stage: ['pptx', 'pptm'], folio: ['pdf'] }

/** The Save as picker's name: the document's own name and type when it has one, else Untitled. */
export function saveAsName(editor, title) {
  const kind = editorOf(editor).id
  const name = String(title ?? '').trim()
  const dot = name.lastIndexOf('.')
  const ext = dot > 0 ? name.slice(dot + 1).toLowerCase() : ''
  if (TYPES[kind].includes(ext)) return saveName(kind, name.slice(0, dot)).replace(/\.[a-z]+$/, `.${ext}`)
  return saveName(kind, name || 'Untitled')
}

/** A whole number from 0 below `count` (0 when there are none). */
export function clampPage(page, count) {
  const n = Math.max(0, Math.floor(Number(count) || 0))
  const p = Math.floor(Number(page) || 0)
  return n === 0 ? 0 : Math.min(Math.max(p, 0), n - 1)
}

/** The page after moving `delta` pages, kept inside the document. */
export function stepPage(page, delta, count) {
  return clampPage(clampPage(page, count) + (Number(delta) || 0), count)
}

/** "Page 2 of 14" (or "Slide 2 of 14"). */
export function pageLabel(page, count, word = 'Page') {
  const n = Math.max(0, Math.floor(Number(count) || 0))
  return n ? `${word} ${clampPage(page, n) + 1} of ${n}` : `No ${word.toLowerCase()}s`
}

/** How the page area moves between pages, read from the editor and its describe:
 *   Ledger  { kind: 'sheets', count, tabs: [{ index, name }] }   a hidden sheet keeps its place, no tab
 *   Stage   { kind: 'slides', count, aspect }                    the thumbnail strip
 *   Scroll, Folio (and the rest)  { kind: 'pages', count }       page up and down, "Page N of M" */
export function pageNav(editor, describe) {
  const d = parseJson(describe) ?? {}
  const kind = editorOf(editor).id
  const whole = (v) => Math.max(0, Math.floor(Number(v) || 0))
  const known = (v) => v !== null && v !== undefined && Number.isFinite(Number(v))
  if (kind === 'ledger' && Array.isArray(d.sheets)) {
    const tabs = d.sheets
      .map((s, i) => ({ index: known(s?.index) ? Number(s.index) : i, name: String(s?.name ?? `Sheet${i + 1}`), state: String(s?.state ?? 'visible') }))
      .filter((s) => s.state === 'visible')
      .map(({ index, name }) => ({ index, name }))
    return { kind: 'sheets', count: d.sheets.length, tabs }
  }
  if (kind === 'stage') {
    const w = Number(d.slide_width_pt)
    const h = Number(d.slide_height_pt)
    const count = known(d.slide_count) ? whole(d.slide_count) : Array.isArray(d.slides) ? d.slides.length : 1
    return { kind: 'slides', count, aspect: w > 0 && h > 0 ? `${w} / ${h}` : '16 / 9' }
  }
  // Folio says page_count (its pages list stops at 500); Scroll says pages; M0's page has one
  const count = known(d.page_count) ? whole(d.page_count) : known(d.pages) ? whole(d.pages) : 1
  return { kind: 'pages', count }
}

/** The id of the newest step still done (null when there is none): the mark an open or a save leaves. */
export function newestDone(history) {
  const done = (history ?? []).filter((s) => !s.undone)
  return done.length ? done.at(-1).id : null
}

/** True when the document changed since its mark. */
export function isDirty(history, mark) {
  return newestDone(history) !== (mark ?? null)
}

/** The editor a file opens in, by its name's extension (null for anything SCRIPTORIUM does not open). */
export function editorForPath(path) {
  const name = String(path ?? '').split(/[\\/]/).pop()
  const dot = name.lastIndexOf('.')
  const ext = dot > 0 ? name.slice(dot + 1).toLowerCase() : ''
  return Object.keys(TYPES).find((k) => TYPES[k].includes(ext)) ?? null
}

/** A path's file name. */
export function fileNameOf(path) {
  return String(path ?? '').split(/[\\/]/).pop()
}

// --- locked PDFs ---

// the crate's own words (folio FolioError's Display) for a PDF that needs a password, and a wrong one
export const PDF_LOCKED = 'locked with a password'
export const PDF_WRONG_PASSWORD = 'password does not open'

/** What an open's failure says about a password: 'needs' (ask for it), 'wrong' (ask again) or null. */
export function pdfLock(error) {
  const text = plainError(error)
  if (text.includes(PDF_WRONG_PASSWORD)) return 'wrong'
  if (text.includes(PDF_LOCKED)) return 'needs'
  return null
}

/** The password dialog's words for a locked file: { title, message, wrong }. */
export function passwordPrompt(lock, name = '') {
  const file = String(name ?? '').trim() || 'This PDF'
  const wrong = lock === 'wrong'
  return {
    title: 'This PDF is locked',
    message: wrong ? `That password does not open ${file}. Try again.` : `${file} is locked with a password. Enter its password to open it.`,
    wrong,
  }
}

// --- links on the page ---

/** A web address the member may open in the browser: { uri, host } (https, http or mailto only), or null. */
export function webLink(uri) {
  const text = String(uri ?? '').trim()
  if (!text || /\s/.test(text)) return null
  let url
  try {
    url = new URL(text)
  } catch {
    return null
  }
  if (url.protocol === 'mailto:') return { uri: text, host: url.pathname || text }
  if (url.protocol !== 'https:' && url.protocol !== 'http:') return null
  return url.hostname ? { uri: text, host: url.hostname } : null
}

/** Where a link on the page goes: { kind: 'page', page } (0-based; the target says #page=N) or { kind: 'web',
 * uri, host }, or null for a target the room does not follow. */
export function linkTarget(target) {
  const text = String(target ?? '').trim()
  const m = /^#page=(\d+)$/.exec(text)
  if (m) return Number(m[1]) >= 1 ? { kind: 'page', page: Number(m[1]) - 1 } : null
  const web = webLink(text)
  return web ? { kind: 'web', ...web } : null
}

// --- fitting the page in the view ---

/** The page's size in points: the editor's state (`width`, `height`) when it says, else the display list's. */
export function pageSizeOf(state, list) {
  const w = Number(state?.width)
  const h = Number(state?.height)
  if (w > 0 && h > 0) return { w, h }
  const lw = Number(list?.w)
  const lh = Number(list?.h)
  return lw > 0 && lh > 0 ? { w: lw, h: lh } : null
}

const SCROLLBAR = 14

/** The zoom (css px per point) for 'page' (all of it) or 'width', in a view { w, h } in css px; null
 * when either size is unknown. */
export function fitZoomFor(mode, size, view) {
  const vw = Number(view?.w)
  const vh = Number(view?.h)
  if (!size || !(size.w > 0) || !(size.h > 0) || !(vw > 0) || !(vh > 0)) return null
  // fitting the width leaves room for the scrollbar the taller page brings
  return mode === 'width' ? fitWidth(size.w, Math.max(1, vw - SCROLLBAR)) : fitPage(size.w, size.h, vw, vh)
}
