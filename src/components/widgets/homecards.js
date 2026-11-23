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
