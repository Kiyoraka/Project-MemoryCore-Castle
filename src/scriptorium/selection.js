// The page's caret and selection, pure: reading the editing ground's answers (a pos is the editor's own
// place, opaque here; a selection is { anchor, focus }) and placing the caret and the selection boxes
// over the canvas with the painter's page layout ({ left, top, zoom }, css px).
import { parseJson } from './room.js'

/** A pos from the host (anything but null). */
export function readPos(value) {
  const v = parseJson(value)
  return v === null || v === undefined ? null : v
}

const num = (v) => (v === null || v === undefined || v === '' ? NaN : Number(v))
const finite = (...vs) => vs.every((v) => Number.isFinite(v))

/** The caret's box { page, x, y, height } (points), or null. */
export function readCaret(value) {
  const c = parseJson(value)
  if (!c || typeof c !== 'object') return null
  const [page, x, y, height] = [num(c.page), num(c.x), num(c.y), num(c.height)]
  if (!finite(page, x, y, height) || page < 0) return null
  return { page: Math.floor(page), x, y, height: Math.max(0, height) }
}

/** The selection's boxes [{ page, x, y, w, h }] (points). */
export function readRects(value) {
  const list = parseJson(value)
  if (!Array.isArray(list)) return []
  return list
    .map((r) => ({ page: num(r?.page), x: num(r?.x), y: num(r?.y), w: num(r?.w), h: num(r?.h) }))
    .filter((r) => finite(r.page, r.x, r.y, r.w, r.h) && r.page >= 0 && r.w > 0 && r.h > 0)
}

/** A copy's { text, rich }. */
export function readClip(value) {
  const c = parseJson(value)
  return { text: typeof c?.text === 'string' ? c.text : '', rich: c?.rich ?? null }
}

/** A caret at `pos` (null for no pos). */
export function selectionAt(pos) {
  return pos === null || pos === undefined ? null : { anchor: pos, focus: pos }
}

/** The selection grown (or started) to `pos`: the anchor stays. */
export function extendTo(sel, pos) {
  if (pos === null || pos === undefined) return sel ?? null
  return { anchor: sel?.anchor ?? pos, focus: pos }
}

/** A selection from the host (`{ anchor, focus? }`), or null. */
export function readSelection(value) {
  const s = parseJson(value)
  if (!s || typeof s !== 'object' || s.anchor === null || s.anchor === undefined) return null
  return { anchor: s.anchor, focus: s.focus === null || s.focus === undefined ? s.anchor : s.focus }
}

/** True when anchor and focus are one place. */
export function isCollapsed(sel) {
  return !sel || JSON.stringify(sel.anchor) === JSON.stringify(sel.focus)
}

/** The items of one page. */
export function onPage(items, page) {
  return (items ?? []).filter((r) => r && r.page === page)
}

/** A box in points -> css px over the canvas { left, top, width, height }. */
export function toScreen(rect, layout) {
  const z = Number(layout?.zoom) || 1
  return { left: rect.x * z + (Number(layout?.left) || 0), top: rect.y * z + (Number(layout?.top) || 0), width: rect.w * z, height: rect.h * z }
}

/** The caret's bar in css px { left, top, height } (at least 1 px tall), or null. */
export function caretBox(caret, layout) {
  if (!caret) return null
  const z = Number(layout?.zoom) || 1
  return { left: caret.x * z + (Number(layout?.left) || 0), top: caret.y * z + (Number(layout?.top) || 0), height: Math.max(1, caret.height * z) }
}

/** The ground's motion for a caret intent ('caret.word_left' -> 'word_left'), or null. */
export function motionOf(intent) {
  const m = /^caret\.(left|right|up|down|word_left|word_right|line_start|line_end|doc_start|doc_end)$/.exec(String(intent ?? ''))
  return m ? m[1] : null
}
