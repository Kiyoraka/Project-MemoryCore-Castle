// HOME's PROFILE tab, its small decisions (Phase 49), pure and tested: the two looks, who hears a
// trait, the traits she lives and the ones waiting for a yes, and where each star sits.

/** The two looks the member chooses between (Settings > Rooms > Profile, and the THEME room). */
export const PROFILE_LOOKS = [
  { id: 'cards', label: 'Cards', line: 'Numbered cards, a small sign for who hears each one.' },
  { id: 'constellation', label: 'Constellation', line: 'Her traits as stars, the one you choose opened beside them.' },
]

export function profileLookOf(value) {
  return PROFILE_LOOKS.some((l) => l.id === value) ? value : 'cards'
}

/** Who may hear a trait in the chat room. */
export const AUDIENCES = [
  { id: 'everyone', label: 'Everyone', line: 'colours how it talks to everyone in the room' },
  { id: 'owner', label: 'Only you', line: 'only when it answers you in the room, never with guests' },
  { id: 'never', label: 'Never in the room', line: 'lived at home, never in the room' },
]

export function audienceOf(id) {
  return AUDIENCES.find((a) => a.id === id) ?? AUDIENCES[0]
}

export const ORIGIN_LABELS = { written: 'Written by you', diary: 'From the diary', companion: 'Proposed by your companion' }

/** The traits she lives, by number. */
export function activeTraits(traits) {
  return (traits ?? []).filter((t) => t.status === 'active').sort((a, b) => a.number - b.number)
}

/**
 * Everything waiting for the member's yes, in one row: the proposals a scan or an idea just
 * brought (not kept yet) first, then the ones the companion proposed with `trait propose` (kept
 * as proposed). Each item says where it came from, so Keep and No know what to do.
 */
export function waitingItems(fresh, traits) {
  const newOnes = (fresh ?? []).map((p, i) => ({ key: `new-${i}`, source: 'new', index: i, ...p }))
  const kept = (traits ?? [])
    .filter((t) => t.status === 'proposed')
    .sort((a, b) => a.number - b.number)
    .map((t) => ({ key: `trait-${t.number}`, source: 'kept', ...t }))
  return [...newOnes, ...kept]
}

/** "changed 2026-11-14 20:15 · From the diary" */
export function traitNote(t) {
  return [t.updatedAt ? `changed ${t.updatedAt}` : '', ORIGIN_LABELS[t.origin] ?? ''].filter(Boolean).join(' · ')
}

// A small steady number from a string, so the same trait always lands in the same place.
function seed(text) {
  let h = 2166136261
  for (const ch of String(text)) h = Math.imul(h ^ ch.codePointAt(0), 16777619)
  return (h >>> 0) / 4294967295
}

/**
 * Where each star sits in a `w` x `h` sky: around an ellipse, in number order, each nudged inward
 * or outward by its own name, so the same traits always draw the same constellation. Waiting stars
 * sit in the lower right corner. Returns [{ key, x, y }] in the order given.
 */
export function starPositions(stars, w, h) {
  const list = stars ?? []
  const lived = list.filter((s) => !s.waiting)
  const waiting = list.filter((s) => s.waiting)
  const cx = w / 2
  const cy = h / 2
  const out = new Map()
  lived.forEach((s, i) => {
    const angle = -Math.PI / 2 + (i / Math.max(lived.length, 1)) * Math.PI * 2 + (lived.length === 1 ? 0 : 0.35)
    const reach = 0.78 + seed(s.name ?? s.key) * 0.22
    out.set(s.key, { key: s.key, x: Math.round(cx + Math.cos(angle) * w * 0.36 * reach), y: Math.round(cy + Math.sin(angle) * h * 0.34 * reach) })
  })
  waiting.forEach((s, i) => {
    out.set(s.key, { key: s.key, x: Math.round(w - 56 - i * 64), y: Math.round(h - 36) })
  })
  return list.map((s) => out.get(s.key))
}
