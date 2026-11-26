// HOME's widgets on a grid (W4). Every widget keeps ONE cell rectangle {x, y, w, h} in cell units of
// a six-column HOME, the same for every theme and kept across restarts (settings `widgetGrid`).
//
// Snap to grid ON (the default, settings `widgetSnap`): whole cells only, and no two widgets ever
// share a cell - a card that is moved or resized stays where it was put and the cards it hits move
// straight down. A narrower HOME (4 or 2 columns) is worked out from the six-column layout in reading
// order and never saved, so going back to full width brings the member's own layout back.
// Snap OFF: any position and size (fractions of a cell), free like before, overlaps allowed.
// Switching snap back on puts every card on its nearest cells and pushes the ones that collide down.
//
// Everything here is pure (no DOM), so the packing is tested on its own (widgetgrid.test.js).

export const COLS = 6
export const ROW_PX = 112
export const GAP_PX = 12
export const MAX_H = 8
// past this a cell stops growing and the grid is centred beside the chat panel
export const MAX_CELL_PX = 220
// the narrowest a cell may be at 6 and at 4 columns before HOME drops to fewer columns
const MIN_CELL_6 = 118
const MIN_CELL_4 = 100

// Each widget's starting span and the smallest span it still shows its content in (snap on). One
// row (112px) holds the clock or the gauges; two hold TODAY's four rows.
const SPANS = {
  clock: { w: 2, h: 1, minW: 1, minH: 1 },
  gauges: { w: 2, h: 1, minW: 2, minH: 1 },
  weather: { w: 2, h: 2, minW: 2, minH: 1 },
  disks: { w: 2, h: 2, minW: 2, minH: 1 },
  sentinel: { w: 2, h: 1, minW: 2, minH: 1 },
  automations: { w: 2, h: 2, minW: 2, minH: 2 },
  calendar: { w: 2, h: 2, minW: 2, minH: 2 },
  sweep: { w: 2, h: 3, minW: 2, minH: 2 },
  usage: { w: 2, h: 2, minW: 2, minH: 1 },
  today: { w: 2, h: 2, minW: 2, minH: 2 },
  projects: { w: 2, h: 2, minW: 2, minH: 2 },
  diary: { w: 2, h: 1, minW: 2, minH: 1 },
  growth: { w: 2, h: 1, minW: 1, minH: 1 },
  backup: { w: 1, h: 1, minW: 1, minH: 1 },
  database: { w: 1, h: 1, minW: 1, minH: 1 },
}
const CREDIT_SPAN = { w: 1, h: 1, minW: 1, minH: 1 }
const OTHER_SPAN = { w: 2, h: 2, minW: 1, minH: 1 }

export function spanOf(id) {
  return SPANS[id] ?? (String(id).startsWith('credit-') ? CREDIT_SPAN : OTHER_SPAN)
}

// Snap on: the size chips (w x h) and the smallest a custom size may be. Snap off: S / M / L set the
// card's box to its starting span times these, and its corner sizes it freely (from one cell).
export const GRID_PRESETS = [[1, 1], [2, 1], [2, 2], [3, 3]]
export const FREE_SIZES = { s: 0.75, m: 1, l: 1.25 }
// the S/M/L zoom an older Castle kept in `widgetSizes`, used once to size a widget's first cells
const OLD_ZOOM = { s: 0.85, m: 1, l: 1.15 }

const EPS = 1e-6
const r2 = (n) => Math.round(n * 20) / 20 // free mode: twentieths of a cell
const num = (n, d = 0) => (Number.isFinite(Number(n)) ? Number(n) : d)

// The grid HOME draws for the room beside the chat panel `width` px wide: 6, 4 or 2 columns.
export function columnsFor(width) {
  if (width >= COLS * MIN_CELL_6 + (COLS - 1) * GAP_PX) return 6
  if (width >= 4 * MIN_CELL_4 + 3 * GAP_PX) return 4
  return 2
}

// Cell width and pitch for that room. `cols` defaults to the columns the width allows; free mode
// always draws six columns (squeezed if HOME is narrow, as the old percent corners were).
export function geometry(width, cols = columnsFor(width)) {
  const w = Math.max(0, num(width))
  const cell = Math.max(1, Math.min(MAX_CELL_PX, (w - (cols - 1) * GAP_PX) / cols))
  const used = cols * cell + (cols - 1) * GAP_PX
  return { cols, cell, gap: GAP_PX, row: ROW_PX, colPitch: cell + GAP_PX, rowPitch: ROW_PX + GAP_PX, width: used, offset: Math.max(0, (w - used) / 2) }
}

// The pixel box of a cell rectangle.
export function boxOf(c, geo) {
  return {
    left: c.x * geo.colPitch,
    top: c.y * geo.rowPitch,
    width: c.w * geo.cell + Math.max(0, c.w - 1) * geo.gap,
    height: c.h * geo.row + Math.max(0, c.h - 1) * geo.gap,
  }
}

export function overlaps(a, b) {
  return a.x < b.x + b.w - EPS && b.x < a.x + a.w - EPS && a.y < b.y + b.h - EPS && b.y < a.y + a.h - EPS
}

// The pairs of ids that share space (none, with snap on).
export function overlapping(list) {
  const pairs = []
  for (let i = 0; i < list.length; i++) for (let j = i + 1; j < list.length; j++) if (overlaps(list[i], list[j])) pairs.push([list[i].id, list[j].id])
  return pairs
}

const readingOrder = (a, b) => a.y - b.y || a.x - b.x || String(a.id).localeCompare(String(b.id))
const copy = (list) => list.map((c) => ({ ...c }))

// A cell rectangle kept on a `cols`-wide grid: whole cells, never below the widget's minimum, never
// past the right edge (snap on); any fraction from one cell up (snap off).
export function clampCell(c, { snap = true, cols = COLS } = {}) {
  const span = spanOf(c.id)
  const q = snap ? Math.round : r2
  const minW = snap ? span.minW : 1
  const minH = snap ? span.minH : 1
  const w = Math.min(cols, Math.max(Math.min(minW, cols), q(num(c.w, span.w))))
  const h = Math.min(MAX_H, Math.max(minH, q(num(c.h, span.h))))
  const x = Math.min(cols - w, Math.max(0, q(num(c.x))))
  const y = Math.max(0, q(num(c.y)))
  return { id: c.id, x, y, w, h }
}

// Moves `o` straight down until it shares no space with any of `placed`.
function dropBelow(o, placed) {
  let p
  while ((p = placed.find((q) => overlaps(q, o)))) o.y = p.y + p.h
  return o
}

// The card `id` stays where it is; every card it hits (and every card those hit) moves down.
export function settle(list, id) {
  const all = copy(list)
  const moved = all.find((c) => c.id === id)
  if (!moved) return pack(all).list
  const placed = [moved]
  for (const o of all.filter((c) => c !== moved).sort(readingOrder)) placed.push(dropBelow(o, placed))
  return all
}

// Snap on: every card to its nearest whole cells (never below its minimum), then in reading order
// any card that now shares space moves down. `moved` and `pushed` count what changed.
export function pack(list, cols = COLS) {
  let moved = 0
  let pushed = 0
  const placed = []
  for (const c of copy(list).sort(readingOrder)) {
    const o = clampCell(c, { snap: true, cols })
    if (['x', 'y', 'w', 'h'].some((k) => o[k] !== c[k])) moved++
    const y0 = o.y
    placed.push(dropBelow(o, placed))
    if (o.y !== y0) pushed++
  }
  return { list: placed, moved, pushed }
}

// "Tidy up": in reading order every card moves up into the gap above it, if there is one.
export function tidy(list) {
  const placed = []
  for (const o of pack(list).list.sort(readingOrder)) {
    while (o.y > 0 && !placed.some((q) => overlaps(q, { ...o, y: o.y - 1 }))) o.y--
    placed.push(o)
  }
  return placed
}

// The first free spot for a w x h card on a `cols`-wide grid, scanning rows from `fromY`.
export function firstFit(placed, w, h, cols = COLS, fromY = 0) {
  const width = Math.min(w, cols)
  for (let y = Math.max(0, Math.floor(fromY)); y < 10000; y++)
    for (let x = 0; x + width <= cols; x++) if (!placed.some((q) => overlaps(q, { x, y, w: width, h }))) return { x, y }
  return { x: 0, y: 0 }
}

// A narrower HOME: the six-column layout in reading order, each card at the first free spot of the
// narrower grid and never wider than it. Worked out every time, never saved.
export function reflow(list, cols) {
  const laid = pack(list).list
  return cols >= COLS ? laid : dense(laid, cols)
}

// Each card in reading order at the first free spot of a `cols`-wide grid.
function dense(list, cols) {
  const placed = []
  for (const c of [...list].sort(readingOrder)) {
    const w = Math.min(c.w, cols)
    placed.push({ ...c, w, ...firstFit(placed, w, c.h, cols) })
  }
  return placed
}

const rowsOf = (list) => list.reduce((m, c) => Math.max(m, c.y + c.h), 0)

// What HOME draws: snap on, the widgets packed with no overlap (reflowed when HOME is narrower);
// snap off, the free places as they are.
export function layoutFor(list, { snap = true, cols = COLS } = {}) {
  if (!snap) return copy(list).map((c) => clampCell(c, { snap: false }))
  return reflow(list, cols)
}

// --- where a widget's FIRST cells come from ------------------------------------------------------

// A CSS length from a theme's place ("12%", "190px", "calc(22% + 190px)") in pixels of `total`.
export function cssPx(value, total) {
  if (value == null) return null
  if (typeof value === 'number') return value
  const text = String(value).trim().replace(/^calc\((.*)\)$/, '$1')
  let px = 0
  let found = false
  const re = /([+-]?)\s*(\d+(?:\.\d+)?)\s*(%|px)?/g
  let m
  while ((m = re.exec(text))) {
    const n = Number(m[2]) * (m[1] === '-' ? -1 : 1)
    px += m[3] === '%' ? (n / 100) * total : n
    found = true
  }
  return found ? px : null
}

// `ctx`: { home: {width, height}, area: {left, top, width, height} } in px - HOME and the grid's room
// beside the chat panel, relative to HOME.
function cellAt(id, leftPx, topPx, ctx, span) {
  const geo = geometry(ctx.area.width, COLS)
  const x = (leftPx - ctx.area.left - geo.offset) / geo.colPitch
  const y = (topPx - ctx.area.top) / geo.rowPitch
  return clampCell({ id, x, y, w: span.w, h: span.h }, { snap: true })
}

// The cells for a corner an older Castle kept (percent of HOME), with its old S/M/L zoom.
export function cellFromOldPlace(id, place, ctx, size = 'm') {
  const span = spanOf(id)
  const zoom = OLD_ZOOM[size] ?? 1
  const sized = { ...span, w: Math.max(span.minW, Math.round(span.w * zoom)), h: Math.max(span.minH, Math.round(span.h * zoom)) }
  return cellAt(id, (num(place.left) / 100) * ctx.home.width, (num(place.top) / 100) * ctx.home.height, ctx, sized)
}

// The cells nearest the theme's own place for a widget (left/right, top/bottom anchors).
export function cellFromTheme(id, place, ctx) {
  const span = spanOf(id)
  if (!place) return null
  const geo = geometry(ctx.area.width, COLS)
  // a card anchored to the right or the bottom keeps that edge (its own span, not the theme's width)
  const width = span.w * geo.cell + (span.w - 1) * geo.gap
  const height = span.h * geo.row + (span.h - 1) * geo.gap
  const left = cssPx(place.left, ctx.home.width) ?? (place.right != null ? ctx.home.width - cssPx(place.right, ctx.home.width) - width : null)
  const top = cssPx(place.top, ctx.home.height) ?? (place.bottom != null ? ctx.home.height - cssPx(place.bottom, ctx.home.height) - height : null)
  if (left == null || top == null || !Number.isFinite(left) || !Number.isFinite(top)) return null
  return cellAt(id, left, top, ctx, span)
}

// The kept grid as a list of cell rectangles (only well-formed ones).
export function cellsOf(grid = {}, ids = null) {
  return Object.entries(grid ?? {})
    .filter(([id, c]) => (!ids || ids.includes(id)) && c && ['x', 'y', 'w', 'h'].every((k) => Number.isFinite(Number(c[k]))))
    .map(([id, c]) => ({ id, x: Number(c.x), y: Number(c.y), w: Number(c.w), h: Number(c.h) }))
}

export function gridOf(list) {
  const grid = {}
  for (const c of list) grid[c.id] = { x: c.x, y: c.y, w: c.w, h: c.h }
  return grid
}

// Every widget in `ids` gets cells. The first time (nothing kept yet, an older Castle's layout, or
// after "Reset the layout") every widget starts at the cells nearest its old corner or the theme's
// place and the whole set is packed, so no widget is lost and none overlaps. Later, a widget that
// appears for the first time (switched on, a new release) takes its theme spot if it is free and
// otherwise the first free spot; the cards already there never move for it.
// `from(id)`: { old, size, theme } - the older Castle's corner and size, and the theme's place.
export function completeGrid(grid = {}, ids = [], from = () => ({}), ctx = null) {
  const kept = cellsOf(grid)
  const missing = ids.filter((id) => !kept.some((c) => c.id === id))
  if (!missing.length) return grid ?? {}
  const live = Boolean(ctx?.area?.width && ctx?.home?.width)
  const oldCell = (id) => {
    const f = from(id) ?? {}
    return live && f.old ? cellFromOldPlace(id, f.old, ctx, f.size) : null
  }
  const themeCell = (id) => (live ? cellFromTheme(id, (from(id) ?? {}).theme, ctx) : null)
  // a widget joining cards already on the grid: its spot if that is free, else the first free spot
  const join = (shown, id, want) => {
    const span = spanOf(id)
    const cell = want && !shown.some((q) => overlaps(q, want)) ? want : { id, w: span.w, h: span.h, ...firstFit(shown, span.w, span.h) }
    shown.push(cell)
    return cell
  }
  if (!kept.length) {
    // An older Castle's layout: the member's cards keep their order and are only pushed apart (no
    // tidying, so a card put low stays low); a widget it had no corner for joins them.
    const olds = missing.map(oldCell).filter(Boolean)
    if (olds.length) {
      const shown = pack(olds).list
      for (const id of missing) if (!shown.some((c) => c.id === id)) join(shown, id, themeCell(id))
      return gridOf(shown)
    }
    // A first start or "Reset the layout": the theme's places, packed and moved up into the gaps
    // its pixel places leave. If they still stack deeper than HOME is tall (a theme that puts six
    // cards in one column), the cards are laid densely in that order instead.
    const placed = []
    const loose = []
    for (const id of missing) {
      const c = themeCell(id)
      if (c) placed.push(c)
      else loose.push(id)
    }
    let packed = tidy(placed)
    const rows = ctx?.area?.height ? Math.floor((ctx.area.height + GAP_PX) / (ROW_PX + GAP_PX)) : Infinity
    if (rowsOf(packed) > rows) packed = dense(packed, COLS)
    for (const id of loose) join(packed, id, null)
    return gridOf(packed)
  }
  const shown = pack(cellsOf(grid, ids)).list
  const all = [...kept]
  for (const id of missing) all.push(join(shown, id, oldCell(id) ?? themeCell(id)))
  return gridOf(all)
}

// --- the member's changes --------------------------------------------------------------------------

// A card moved to (x, y): snap on, to whole cells with the cards it hits moved down; off, anywhere.
export function moveTo(list, id, x, y, { snap = true } = {}) {
  const next = copy(list).map((c) => (c.id === id ? clampCell({ ...c, x, y }, { snap }) : c))
  return snap ? settle(next, id) : next
}

// A card given a new size (w x h cells), kept on HOME: snap on, the cards it now covers move down.
export function resizeTo(list, id, w, h, { snap = true } = {}) {
  const next = copy(list)
  let c = next.find((o) => o.id === id)
  if (!c) {
    // not on HOME yet: it waits under everything else
    const bottom = next.reduce((m, o) => Math.max(m, o.y + o.h), 0)
    c = { id, x: 0, y: bottom, w, h }
    next.push(c)
  }
  const sized = clampCell({ ...c, w, h }, { snap })
  // a wider card moves left rather than past the right edge
  Object.assign(c, sized, { x: Math.min(sized.x, COLS - sized.w) })
  return snap ? settle(next, id) : next
}

// The sizes the member can choose for a widget: snap on, the w x h chips (greyed below its
// minimum); snap off, S / M / L.
export function sizeChoices(id, snap) {
  const span = spanOf(id)
  if (snap) return GRID_PRESETS.map(([w, h]) => ({ key: `${w}x${h}`, label: `${w}×${h}`, w, h, ok: w >= span.minW && h >= span.minH }))
  return Object.entries(FREE_SIZES).map(([key, f]) => ({ key, label: key.toUpperCase(), w: r2(span.w * f), h: r2(span.h * f), ok: true }))
}

// Which chip is pressed for a card's size, or '' (a custom size).
export function chosenSize(id, cell, snap) {
  if (!cell) return ''
  return sizeChoices(id, snap).find((s) => Math.abs(s.w - cell.w) < EPS && Math.abs(s.h - cell.h) < EPS)?.key ?? ''
}

// The settings change for the grid as it now is (hidden widgets keep their cells).
export function gridPatch(grid, changed = []) {
  const next = { ...(grid ?? {}) }
  for (const c of changed) next[c.id] = { x: c.x, y: c.y, w: c.w, h: c.h }
  return { widgetGrid: next }
}

// Snap switched on or off. On: every kept card goes to its nearest cells and the ones that collide
// move down, so nothing overlaps from then on.
// Returns { patch, moved, pushed }.
export function snapPatch(grid, on) {
  if (!on) return { patch: { widgetSnap: false }, moved: 0, pushed: 0 }
  const { list, moved, pushed } = pack(cellsOf(grid))
  return { patch: { widgetSnap: true, widgetGrid: gridOf(list) }, moved, pushed }
}

export function snapOf(settings = {}) {
  return settings.widgetSnap !== false
}
