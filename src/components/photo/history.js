// PHOTO's undo pills: every edit is one pill under the photo, and each pill undoes itself alone.
// The state is { doc, pixels } (pixels: { [layerId]: canvas }, never changed in place - an edit makes
// a new canvas, so a pill can keep the old one). Pure, so it can be tested.
//
// Pill kinds:
//   adjust        { before: { slider: old value } }       puts back only the sliders it changed
//   layer-new     { layerId, shown: [ids it hid] }        the layer goes, the hidden originals show
//   layer-change  { layerId, before: { field: old } }     the layer's old fields come back
//   layer-delete  { layer, index, pixel }                 the layer comes back where it was
//   order         { before: [ids bottom to top] }         the old order
//   pixels        { layerId, before: canvas }             the layer as it was before the stroke
//   select        { before: selection }                   the old selection
//   snapshot      { before: { doc, pixels, selection } }  crop, rotate, flip, resize: the state before
//                                                          it, and every pill after it is dropped

export const MAX_PILLS = 60

let counter = 0
export function pillId() {
  counter += 1
  return `p${counter}`
}

/** A pill added at the end (the oldest goes past MAX_PILLS). */
export function addPill(history, pill) {
  const next = [...history, { id: pill.id ?? pillId(), ...pill }]
  return next.length > MAX_PILLS ? next.slice(next.length - MAX_PILLS) : next
}

const withLayers = (doc, layers) => ({ ...doc, layers })

/** Undoes one pill alone. -> { state, history } (unchanged when the pill is not there). */
export function undoPill(state, history, id) {
  const at = history.findIndex((p) => p.id === id)
  if (at < 0) return { state, history }
  const pill = history[at]
  const rest = history.filter((p) => p.id !== id)
  const { doc, pixels } = state
  switch (pill.kind) {
    case 'adjust':
      return { state: { doc: { ...doc, adjust: { ...doc.adjust, ...pill.before } }, pixels }, history: rest }
    case 'layer-new': {
      const layers = doc.layers.filter((l) => l.id !== pill.layerId).map((l) => (pill.shown?.includes(l.id) ? { ...l, visible: true } : l))
      const nextPixels = { ...pixels }
      delete nextPixels[pill.layerId]
      // what was done to that layer afterwards has nothing left to undo
      return { state: { doc: withLayers(doc, layers), pixels: nextPixels }, history: rest.filter((p, i) => i < at || p.layerId !== pill.layerId) }
    }
    case 'layer-change':
      return { state: { doc: withLayers(doc, doc.layers.map((l) => (l.id === pill.layerId ? { ...l, ...pill.before } : l))), pixels }, history: rest }
    case 'layer-delete': {
      if (doc.layers.some((l) => l.id === pill.layer.id)) return { state, history: rest }
      const layers = [...doc.layers]
      layers.splice(Math.min(pill.index, layers.length), 0, pill.layer)
      const nextPixels = pill.pixel ? { ...pixels, [pill.layer.id]: pill.pixel } : pixels
      return { state: { doc: withLayers(doc, layers), pixels: nextPixels }, history: rest }
    }
    case 'order': {
      const byId = new Map(doc.layers.map((l) => [l.id, l]))
      const known = pill.before.filter((lid) => byId.has(lid)).map((lid) => byId.get(lid))
      const added = doc.layers.filter((l) => !pill.before.includes(l.id))
      return { state: { doc: withLayers(doc, [...known, ...added]), pixels }, history: rest }
    }
    case 'pixels': {
      if (!(pill.layerId in pixels)) return { state, history: rest }
      // a later stroke on the same layer was drawn over this one; it goes with it
      const kept = rest.filter((p, i) => i < at || !(p.kind === 'pixels' && p.layerId === pill.layerId))
      return { state: { doc, pixels: { ...pixels, [pill.layerId]: pill.before } }, history: kept }
    }
    case 'select':
      return { state: { doc: { ...doc, selection: pill.before ?? null }, pixels }, history: rest }
    case 'snapshot':
      return { state: { doc: pill.before.doc, pixels: pill.before.pixels }, history: history.slice(0, at) }
    default:
      return { state, history: rest }
  }
}

/** Ctrl+Z: the last pill undone; `redo` keeps what Ctrl+Y brings back. */
export function undoLast(state, history) {
  const last = history[history.length - 1]
  if (!last) return { state, history, redo: null }
  const undone = undoPill(state, history, last.id)
  return { ...undone, redo: { state, history } }
}

/** A short label: at most 28 characters. */
export function pillLabel(text) {
  const t = String(text ?? '').replace(/\s+/g, ' ').trim()
  return t.length > 28 ? `${t.slice(0, 27)}…` : t
}
