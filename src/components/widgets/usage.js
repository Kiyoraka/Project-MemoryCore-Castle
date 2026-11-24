// HOME's USAGE card: how a CLI's limit window reads as words and a bar.

// A window whose reset time has passed has started over, even if the CLI has not said so yet.
// Codex only reports its limits while it is being used, so this happens often.
export function windowNow(window, now = Date.now()) {
  if (!window) return null
  if (window.resetsAt && window.resetsAt <= now) return { usedPercent: 0, resetsAt: null, startedOver: true }
  return { usedPercent: window.usedPercent, resetsAt: window.resetsAt ?? null, startedOver: false }
}

// calm under 80%, warm from 80%, full at 100%
export function level(usedPercent) {
  if (usedPercent >= 100) return 'full'
  if (usedPercent >= 80) return 'warm'
  return 'calm'
}

// "resets 3:57 AM" today, "resets Fri 3:10 AM" later in the week.
export function resetWords(resetsAt, now = Date.now()) {
  if (!resetsAt) return ''
  const when = new Date(resetsAt)
  const time = when.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' })
  const sameDay = new Date(now).toDateString() === when.toDateString()
  return sameDay ? `resets ${time}` : `resets ${when.toLocaleDateString('en-US', { weekday: 'short' })} ${time}`
}

// "as of 2:10 AM" when the numbers are over ten minutes old; nothing when fresh.
export function staleWords(observedAt, now = Date.now()) {
  if (!observedAt || now - observedAt < 10 * 60000) return ''
  return `as of ${new Date(observedAt).toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' })}`
}

const STATUS = {
  not_installed: 'Not installed',
  signed_out: 'Sign in to see usage',
  // Codex with an API key: no weekly limit, OpenAI bills each use
  api_key: 'API key · billed per use',
  // Gemini and Grok: Castle cannot read their limits
  not_measured: 'Usage is not measured for this provider',
  unknown: 'Usage not available yet',
}

// What the card shows for one CLI: a line of words, or two bars.
export function rows(cliUsage, now = Date.now()) {
  if (cliUsage.status !== 'ok') return { message: STATUS[cliUsage.status] ?? STATUS.unknown, bars: [] }
  const bars = [
    ['5h', windowNow(cliUsage.fiveHour, now)],
    [cliUsage.weeklyModel ? `Week · ${cliUsage.weeklyModel}` : 'Week', windowNow(cliUsage.weekly, now)],
  ]
    .filter(([, w]) => w)
    .map(([label, w]) => ({
      label,
      percent: Math.round(w.usedPercent),
      level: level(w.usedPercent),
      note: w.startedOver ? 'started over' : resetWords(w.resetsAt, now),
    }))
  return { message: bars.length ? '' : STATUS.unknown, bars, stale: staleWords(cliUsage.observedAt, now) }
}
