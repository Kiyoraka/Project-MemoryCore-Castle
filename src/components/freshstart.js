// The home chat's fresh start card (fresh_start.rs): when the companion's memory of this talk is
// nearly full she asks, in her own words, whether to start fresh. The home chat is never compacted.
// Pure: the panel feeds it the context meter's percent and whether she is working.

export const ASK_AT = 85
export const ASK_AGAIN_AT = 95
// a new session (the meter falls back) watches again
export const REARM_BELOW = 50

// watching -> asking -> (Not now) snoozed -> asking-again -> (Not now) quiet -> (new session) watching
export function nextCard(state, { percent, busy, running }) {
  if (typeof percent !== 'number') return state
  if (percent < REARM_BELOW && state !== 'running') return 'watching'
  if (!running || busy) return state
  if (state === 'watching' && percent >= ASK_AT) return 'asking'
  if (state === 'snoozed' && percent >= ASK_AGAIN_AT) return 'asking-again'
  return state
}

export function notNow(state) {
  if (state === 'asking') return 'snoozed'
  if (state === 'asking-again') return 'quiet'
  return state
}

export const showsCard = (state) => state === 'asking' || state === 'asking-again'

// Her words on the card (first person: the card sits in her own panel).
export function cardWords(percent) {
  const full = typeof percent === 'number' ? ` (${percent}%)` : ''
  return `My memory of this talk is getting full${full}. Shall I start fresh? Nothing is lost: Chat keeps every word, I keep my memory, and I carry our latest talk with me.`
}

// The resting screen's Start fresh: after a fresh start whose new session never got going (the
// watchdog in fresh_start.rs ended it), it tries that fresh start again, the latest talk handed
// over ('retry'); otherwise it is the one-off new talk ('new'), or not there at all (null).
export function restingFresh({ offersFresh, freshRetry }) {
  if (freshRetry) return 'retry'
  return offersFresh ? 'new' : null
}
