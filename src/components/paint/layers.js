// PAINT's layers, bottom first: the member's raster layers ({ owner: 'member' }, their pixels on a
// canvas the room keeps) and the companion's SVG layers ({ owner: 'companion', svg }), each thing
// on its own named layer. The rules - never delete the member's last own layer, the companion only
// ever changes its own layers - live here, pure, so they are tested without a page.

let counter = 0
/** A fresh layer id. */
export const newId = () => `p${++counter}`

export const memberLayers = (layers) => layers.filter((l) => l.owner === 'member')

/** A layer may go unless it is the member's last own layer. */
export function canDelete(layers, id) {
  const layer = layers.find((l) => l.id === id)
  if (!layer) return false
  return layer.owner === 'companion' || memberLayers(layers).length > 1
}

export function deleteLayer(layers, id) {
  return canDelete(layers, id) ? layers.filter((l) => l.id !== id) : layers
}

/** One step up (dir 1, nearer the top) or down (dir -1). */
export function moveLayer(layers, id, dir) {
  const i = layers.findIndex((l) => l.id === id)
  const j = i + dir
  if (i < 0 || j < 0 || j >= layers.length) return layers
  const next = [...layers]
  ;[next[i], next[j]] = [next[j], next[i]]
  return next
}

/** A plain name: one line, at most 60 characters. */
export const plainName = (name) => String(name ?? '').replace(/[\u0000-\u001f<>]/g, ' ').replace(/\s+/g, ' ').trim().slice(0, 60)

/** The companion's name as a layer prefix, as paint.rs makes it: plain, short, never empty. */
export function companionOf(name) {
  const one = String(name ?? '').replace(/[\u0000-\u001f]/g, ' ').replace(/\s+/g, ' ').trim().slice(0, 40)
  return one.replace(/[<>"/\\]/g, '').trim() || 'Companion'
}

/** A companion layer's full name: "<companion> - <thing>", whatever prefix was written. */
export function companionLayerName(companion, given) {
  const prefix = `${companion} - `
  let thing = plainName(given)
  if (thing.startsWith(prefix)) thing = thing.slice(prefix.length)
  else if (thing.startsWith(`${companion}-`)) thing = thing.slice(companion.length + 1)
  thing = thing.replace(/["\\]/g, '').trim().slice(0, 48)
  return thing ? prefix + thing : ''
}

/** A rename: the member's layers take any plain name, the companion's keep its prefix. */
export function renameLayer(layers, id, name, companion) {
  return layers.map((l) => {
    if (l.id !== id) return l
    const next = l.owner === 'companion' ? companionLayerName(companion, name) : plainName(name)
    return next ? { ...l, name: next } : l
  })
}

/** The next free "Layer N" for a new member layer. */
export function nextLayerName(layers) {
  const taken = new Set(layers.map((l) => l.name))
  let n = memberLayers(layers).length
  while (taken.has(`Layer ${n}`) || n < 1) n++
  return `Layer ${n}`
}

/** A layer put just above `aboveId` (or on top). */
export function insertAbove(layers, layer, aboveId) {
  const i = layers.findIndex((l) => l.id === aboveId)
  if (i < 0) return [...layers, layer]
  return [...layers.slice(0, i + 1), layer, ...layers.slice(i + 1)]
}

/**
 * The companion's changes applied: add (or replace when the name is already its own), replace (or
 * add), remove. Only the companion's layers are ever touched; a member layer with the same name is
 * left alone. `clean(svg, id)` returns the drawing to keep, or null when it cannot be kept.
 * Returns { layers, changed: [ids drawn or redrawn], dropped: [names left out] }.
 */
export function applyOps(layers, ops, companion, clean) {
  let next = [...layers]
  const changed = []
  const dropped = []
  for (const op of (ops ?? []).slice(0, 12)) {
    const name = companionLayerName(companion, op?.layer)
    if (!name) continue
    const i = next.findIndex((l) => l.owner === 'companion' && l.name === name)
    if (op.op === 'remove') {
      if (i >= 0) next = next.filter((_, k) => k !== i)
      continue
    }
    if (op.op !== 'add' && op.op !== 'replace') continue
    const id = i >= 0 ? next[i].id : newId()
    const svg = clean(op.svg, id)
    if (!svg) {
      dropped.push(name)
      continue
    }
    if (i >= 0) next = next.map((l, k) => (k === i ? { ...l, svg, visible: true } : l))
    else next = [...next, { id, name, owner: 'companion', visible: true, svg }]
    if (!changed.includes(id)) changed.push(id)
  }
  return { layers: next, changed, dropped }
}

/** What the companion's hand is told about the layers (the drawings go with its own only). */
export const contextOf = (layers) => layers.map((l) => ({ name: l.name, owner: l.owner, visible: !!l.visible, svg: l.owner === 'companion' ? l.svg : '' }))

/** The companion's words, with a note for every drawing the page could not keep. */
export function replyLine(say, dropped) {
  const words = String(say ?? '').trim()
  if (!dropped.length) return words || 'Done.'
  const note = `I could not keep the drawing for ${dropped.map((n) => `"${n}"`).join(', ')}, so I left it out.`
  return words.includes(note) ? words : `${words} ${note}`.trim()
}

/** A painting's default name, from the date. */
export function defaultName(now = new Date()) {
  const pad = (n) => String(n).padStart(2, '0')
  return `Painting ${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())} ${pad(now.getHours())}${pad(now.getMinutes())}`
}
