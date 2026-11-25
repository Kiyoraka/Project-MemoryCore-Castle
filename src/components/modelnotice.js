// The one line a session shows when it started on another model because its own weekly share is
// spent (Rust: session::ModelNotice - { share, model, untilMs }).

const MODEL_NAMES = { opus: 'Opus', sonnet: 'Sonnet' }

/** "Thu 27 Nov, 5:00 AM" in the member's own clock. */
export function untilWords(untilMs, locale) {
  const when = new Date(untilMs)
  const day = when.toLocaleDateString(locale, { weekday: 'short', day: 'numeric', month: 'short' })
  const time = when.toLocaleTimeString(locale, { hour: 'numeric', minute: '2-digit' })
  return `${day}, ${time}`
}

/** The line, or '' when there is no notice (or it has already passed). */
export function noticeWords(notice, companion, now = Date.now(), locale = undefined) {
  if (!notice?.share || !notice.model || !(notice.untilMs > now)) return ''
  const model = MODEL_NAMES[notice.model] ?? notice.model
  return `The ${notice.share} weekly share is spent until ${untilWords(notice.untilMs, locale)}: ${companion} is thinking with ${model} until then.`
}

/** A notice closed by the member stays closed for that share and reset only. */
export const noticeKey = (notice) => (notice ? `${notice.share}@${notice.untilMs}` : '')

// The watch's line for a full session (unstick.rs, event `session-full`: { slot, line }): the line
// when it is this panel's (slot null for the home chat, a number for a DEV tab), else null.
export function fullLineFor(payload, slot) {
  if (!payload || typeof payload.line !== 'string') return null
  const theirs = payload.slot ?? null
  return theirs === (slot ?? null) ? payload.line : null
}
