// HOME's TODAY and PROJECTS cards list every item (the list scrolls inside the card) and say how
// many there are in the header, so nothing is hidden behind a fixed cut.

const COUNT_WORDS = { today: 'open', projects: 'active' }

/** Every item, in order: the card scrolls rather than cutting the list. */
export function cardItems(items) {
  return Array.isArray(items) ? items : []
}

/** The header's count ("7 open", "3 active"), or '' when the card is empty or has no count. */
export function cardCount(kind, items) {
  const word = COUNT_WORDS[kind]
  const n = cardItems(items).length
  return word && n ? `${n} ${word}` : ''
}

/**
 * TODAY's tick: a reminder ticked on the card is closed at once and stays on the card, struck
 * through with an Undo, for `wait` ms; then `settle` reloads HOME and it is gone. Undo in that
 * time opens it again. `pending` holds the ticked ids (a reactive Map in the card).
 */
export function ticker(setDone, settle, wait = 6000, pending = new Map()) {
  return {
    pending,
    has: (id) => pending.has(id),
    async tick(id) {
      if (id == null || pending.has(id)) return
      await setDone(id, true)
      pending.set(id, setTimeout(() => { pending.delete(id); settle() }, wait))
    },
    async undo(id) {
      if (!pending.has(id)) return
      clearTimeout(pending.get(id))
      pending.delete(id)
      await setDone(id, false)
      settle()
    },
  }
}
