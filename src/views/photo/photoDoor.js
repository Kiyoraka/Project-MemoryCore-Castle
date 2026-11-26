// PHOTO and DEV (Phase 235, his word Nov 26 2026: "it can even be wired with dev to adjust photo right ?"): the
// pure parts of a picture handed in from a project - which format it goes back in, the words a project's
// terminal reads, and the rule that a door edit is native only (no redraw from a terminal).

/** The format a picture is saved back in, from its file name: png, jpeg or webp; null for anything else. */
export function formatForPath(path) {
  const ext = String(path ?? '').toLowerCase().match(/\.([a-z0-9]+)$/)?.[1] ?? ''
  if (ext === 'png') return 'png'
  if (ext === 'jpg' || ext === 'jpeg') return 'jpeg'
  if (ext === 'webp') return 'webp'
  return null
}

/** The api her ask uses on a door edit: her reading as always, but a redraw is refused (native tools only). */
export function nativeOnly(api) {
  return {
    ...api,
    photoGenerate: async () => {
      throw new Error('A redraw is not done for a project terminal - only the native tools.')
    },
  }
}

/** What the terminal's answer says: her words, or why it could not be done. */
export function doorDone({ id, file }, outcome) {
  if (outcome?.ok) return { id, file, ok: true, say: String(outcome.say || 'Edited and saved back.').slice(0, 500), reason: null }
  return { id, file, ok: false, say: null, reason: String(outcome?.reason || 'The edit could not be made.').slice(0, 500) }
}

/** A request PHOTO can act on: a kind, a picture path, and for a door an id and words. */
export function readRequest(request) {
  if (!request || typeof request !== 'object' || !formatForPath(request.path)) return null
  if (request.kind === 'open') return { kind: 'open', path: request.path, saveBack: Boolean(request.saveBack) }
  if (request.kind === 'door' && /^[A-Za-z0-9-]{1,40}$/.test(request.id ?? '') && String(request.say ?? '').trim()) {
    return { kind: 'door', path: request.path, id: request.id, say: String(request.say).trim() }
  }
  return null
}

// The requests already handled, by their `at`: kept here, outside the room, so closing PHOTO and opening it
// again never runs a door edit twice.
const handled = new Set()
export function takeOnce(at) {
  if (at == null || handled.has(at)) return false
  handled.add(at)
  return true
}
