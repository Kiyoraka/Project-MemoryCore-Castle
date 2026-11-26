// The Translate Lens's overlay, the pure parts: the helper's picture boxes scaled onto the see-through
// window that sits exactly over the game, how big a bubble's words may be, where a bubble goes (above
// its words with the tail down when there is room, else below with the tail up, always inside the
// window) and which line the lens ring circles.

/** Space between a bubble and the words it points at (the tail's length). */
export const TAIL = 14
/** A bubble keeps this far from the window's edges when the window is big enough. */
export const EDGE = 8
/** The tail never sits closer than this to a bubble's own corner. */
export const TAIL_PAD = 18
export const SIZE_MAX = 22
export const SIZE_MIN = 11
export const LINE_HEIGHT = 1.3

const num = (v, fallback = 0) => (Number.isFinite(Number(v)) ? Number(v) : fallback)

/** lo..hi, and lo when the range is empty (a window smaller than the thing placed in it). */
const clamp = (v, lo, hi) => (hi < lo ? lo : Math.min(hi, Math.max(lo, v)))

/** One line's box from picture pixels to window pixels. */
export function scaleBox(line, picW, picH, winW, winH) {
  const sx = num(picW) > 0 ? num(winW) / num(picW) : 1
  const sy = num(picH) > 0 ? num(winH) / num(picH) : 1
  return { x: num(line?.x) * sx, y: num(line?.y) * sy, w: num(line?.w) * sx, h: num(line?.h) * sy }
}

// CJK, kana, hangul and full-width forms are about one em wide; everything else about half.
const WIDE = /[ᄀ-ᅟ⺀-꓏가-힣豈-﫿︰-﹏＀-｠￠-￦]/u

/** About how wide a text is at a font size, in px (one line, no wrapping). */
export function textWidth(text, size) {
  let ems = 0
  for (const ch of String(text ?? '')) ems += WIDE.test(ch) ? 1 : ch === ' ' ? 0.3 : 0.56
  return ems * size
}

/** About how many lines a text wraps to at a size inside a width. */
export function lineCount(text, size, maxWidth) {
  const width = textWidth(text, size)
  if (width <= 0) return 1
  return Math.max(1, Math.ceil(width / Math.max(1, num(maxWidth, 1))))
}

/** The biggest font size (22 down to 11 px) at which the text fits maxLines inside maxWidth. */
export function fitSize(text, maxWidth, maxLines = 3) {
  for (let size = SIZE_MAX; size > SIZE_MIN; size -= 1) {
    if (lineCount(text, size, maxWidth) <= Math.max(1, maxLines)) return size
  }
  return SIZE_MIN
}

/**
 * Where a bubble of this size goes for a box: above it with the tail down when there is room, else
 * below with the tail up (or whichever side has more room when neither fits), clamped inside the
 * window. tailX is measured from the bubble's left edge and points at the box's centre.
 */
export function bubblePlace(box, bubbleW, bubbleH, winW, winH) {
  const bw = Math.max(0, num(bubbleW))
  const bh = Math.max(0, num(bubbleH))
  const ww = Math.max(0, num(winW))
  const wh = Math.max(0, num(winH))
  const bx = num(box?.x)
  const by = num(box?.y)
  const cx = bx + num(box?.w) / 2
  const edgeX = bw + 2 * EDGE <= ww ? EDGE : 0
  const edgeY = bh + 2 * EDGE <= wh ? EDGE : 0

  const aboveTop = by - TAIL - bh
  const belowTop = by + num(box?.h) + TAIL
  const roomAbove = aboveTop >= edgeY
  const roomBelow = belowTop + bh <= wh - edgeY
  let tail
  if (roomAbove) tail = 'down'
  else if (roomBelow) tail = 'up'
  else tail = by >= wh - (by + num(box?.h)) ? 'down' : 'up'

  const top = clamp(tail === 'down' ? aboveTop : belowTop, edgeY, wh - edgeY - bh)
  const left = clamp(cx - bw / 2, edgeX, ww - edgeX - bw)
  const tailX = clamp(cx - left, Math.min(TAIL_PAD, bw / 2), Math.max(bw - TAIL_PAD, bw / 2))
  return { left, top, tail, tailX }
}

/** The index of the newest line - the lowest one on screen (a dialogue box), the later one on a tie; -1 for none. */
export function newest(lines) {
  let best = -1
  let bottom = -Infinity
  ;(lines ?? []).forEach((line, i) => {
    const b = num(line?.y) + num(line?.h)
    if (b >= bottom) {
      bottom = b
      best = i
    }
  })
  return best
}

/** The lens ring over a box: a circle a little taller than the words, centred on them. */
export function lensRing(box, winW, winH) {
  const size = clamp(Math.max(num(box?.h) * 2.2, 90), 80, Math.max(80, Math.min(220, num(winW), num(winH))))
  return {
    size,
    left: num(box?.x) + num(box?.w) / 2 - size / 2,
    top: num(box?.y) + num(box?.h) / 2 - size / 2,
  }
}

/** A Translation's lines that have words to show, each with its own index kept. */
export function shownLines(translation) {
  return (translation?.lines ?? [])
    .map((line, index) => ({ ...line, index }))
    .filter((line) => String(line.translated ?? '').trim())
}
