// The slide show's pure parts: which slide a key or a click goes to, the letterbox a slide is drawn in,
// and the display list with the editing-only prompt text taken out. Nothing here knows any host.

// prompt text in empty placeholders is drawn with this alpha (render::PROMPT_ALPHA = 0xFE): the show
// leaves those glyph runs out
export const PROMPT_ALPHA_HEX = 'fe'

/** The display list (object or JSON text) as an object without the placeholders' prompt text. */
export function showList(list) {
  let l = list
  if (typeof l === 'string') {
    try {
      l = JSON.parse(l)
    } catch {
      return null
    }
  }
  if (!l || typeof l !== 'object') return null
  const items = Array.isArray(l.items) ? l.items : []
  const kept = items.filter((it) => !(it && it.t === 'glyphs' && typeof it.c === 'string' && it.c.length === 9 && it.c.slice(7).toLowerCase() === PROMPT_ALPHA_HEX))
  return { ...l, items: kept }
}

/** Where the show goes for a key: { to } (a slide index, or `count` for the black end screen), { end: true }
 * to leave, { digit } while a number is being typed, or null for a key the show ignores. */
export function showKey(key, at, count, typed = '') {
  const k = String(key ?? '')
  const last = Math.max(0, count - 1)
  if (/^[0-9]$/.test(k)) return { digit: typed + k }
  if (k === 'Enter' && typed) {
    const n = Number(typed)
    return { to: Math.min(Math.max(1, n || 1), count) - 1 }
  }
  switch (k) {
    case 'Escape':
    case 'Esc':
    case '-':
      return { end: true }
    case 'ArrowRight':
    case 'ArrowDown':
    case 'PageDown':
    case ' ':
    case 'Spacebar':
    case 'Enter':
    case 'n':
    case 'N':
      return at >= count ? { end: true } : { to: Math.min(at + 1, count) }
    case 'ArrowLeft':
    case 'ArrowUp':
    case 'PageUp':
    case 'Backspace':
    case 'p':
    case 'P':
      return { to: Math.max(0, Math.min(at, count) - 1) }
    case 'Home':
      return { to: 0 }
    case 'End':
      return { to: last }
    default:
      return null
  }
}

/** A click: the left button advances (past the last slide the black screen, then the end), the right
 * one goes back. */
export function showClick(button, at, count) {
  if (button === 2) return { to: Math.max(0, Math.min(at, count) - 1) }
  if (at >= count) return { end: true }
  return { to: Math.min(at + 1, count) }
}

/** Where a slide of w x h points is drawn in a view of vw x vh css px: { x, y, scale } (letterboxed). */
export function letterbox(w, h, vw, vh) {
  if (!(w > 0) || !(h > 0) || !(vw > 0) || !(vh > 0)) return { x: 0, y: 0, scale: 1 }
  const scale = Math.min(vw / w, vh / h)
  return { x: (vw - w * scale) / 2, y: (vh - h * scale) / 2, scale }
}

/** The slide the show starts on: 0 from the beginning, else the current one (clamped). */
export function startAt(from, current, count) {
  if (from === 'start' || from === 'beginning') return 0
  const c = Number(current) || 0
  return Math.min(Math.max(0, c), Math.max(0, count - 1))
}
