// WORLDSMITH (S-1126-7 / S-1126-9, his pick worldsmith-9): the pure parts of the page - the helper's
// answers read into what the canvas, the dock and the details table show.

/** The words under the helper card, before the member presses Get it. */
export const HELPER_LINE =
  'WORLDSMITH needs a small helper that reads and changes numbers in the one game you pick - only your own offline single-player games, never online or anti-cheat ones. Antivirus apps sometimes flag tools like this; it touches nothing else.'

/** The message a screenshot goes to the session with. */
export const shotWord = (path) => `Here is a new screenshot of the game: ${path}`

/** The helper's games answer, read: refused ones last, each with its one line, remembered ones marked. */
export function readGames(answer, remembered = []) {
  const known = new Set((remembered ?? []).map((g) => `${String(g.exe).toLowerCase()}|${g.stamp ?? ''}`))
  return (answer?.games ?? [])
    .map((g) => ({
      pid: Number(g.pid),
      exe: String(g.exe ?? ''),
      title: String(g.title || g.exe || ''),
      stamp: String(g.stamp ?? ''),
      refused: Boolean(g.refused),
      reason: String(g.reason ?? ''),
      remembered: known.has(`${String(g.exe ?? '').toLowerCase()}|${g.stamp ?? ''}`),
    }))
    .filter((g) => Number.isFinite(g.pid) && g.pid > 0)
    .sort((a, b) => Number(a.refused) - Number(b.refused) || Number(b.remembered) - Number(a.remembered) || a.title.localeCompare(b.title))
}

/** The state answer, read: every label with its steps and its best address, and the frozen ones. */
export function readState(answer) {
  const freezes = new Map((answer?.freezes ?? []).map((f) => [String(f.address).toUpperCase(), f]))
  const labels = (answer?.labels ?? []).map((l) => {
    const top = (l.top ?? [])[0] ?? null
    const address = top ? String(top.address).toUpperCase().replace(/^0X/, '0x') : ''
    return {
      label: String(l.label),
      steps: (l.steps ?? []).map(Number),
      found: (l.steps ?? []).at(-1) === 1,
      address,
      type: top?.type ?? '',
      value: top?.value ?? '',
      locked: address ? freezes.has(address.toUpperCase()) : false,
    }
  })
  return { attached: answer?.attached ?? null, labels }
}

/** A label's step trail as the tag shows it: 1,284 → 3 → 1. */
export function stepsLine(steps) {
  return (steps ?? []).map((n) => Number(n).toLocaleString('en-US')).join(' → ')
}

/** Where a tag sits on the canvas, from the value's saved place (0 to 1); unplaced ones line up on the left. */
export function tagPlaces(labels, values) {
  const pins = new Map((values ?? []).map((v) => [v.label, v]))
  let free = 0
  return (labels ?? []).map((l) => {
    const pin = pins.get(l.label)
    const placed = pin && Number.isFinite(pin.pinX) && Number.isFinite(pin.pinY)
    const x = placed ? clamp(pin.pinX) : 0.03
    const y = placed ? clamp(pin.pinY) : clamp(0.06 + 0.11 * free++)
    return { ...l, x, y, placed: Boolean(placed) }
  })
}

const clamp = (v) => Math.min(0.97, Math.max(0.03, Number(v)))

/** The helper's freeze or unfreeze arguments for one tag's Lock. */
export function lockCall(tag) {
  if (!tag?.address || !tag.type) return null
  return tag.locked ? ['unfreeze', [tag.address]] : ['freeze', [tag.address, tag.type, String(tag.value)]]
}

/** What a recipe press asks the companion. */
export const recipeWord = (recipe) => `Please use the recipe "${recipe.name}" (${recipe.action} ${recipe.labels}${recipe.amount ? ` to ${recipe.amount}` : ''}).`

/** The words beside the helper card for its state. */
export function helperWord(state) {
  if (!state) return ''
  if (!state.windows) return 'WORLDSMITH works on Windows only.'
  if (state.installed) return ''
  if (!state.released) return "The helper isn't released yet - it is published soon."
  return HELPER_LINE
}
