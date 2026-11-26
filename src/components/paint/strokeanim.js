// How the companion's new strokes draw themselves: every new path in order, each taking a share
// of about 2.4 seconds that follows its length (a short one never vanishes in a blink), and the
// fills fading in after the last stroke. Pure, so it is tested without a page.

export const TOTAL_MS = 2400
export const FILL_MS = 500

/**
 * The plan for paths of `lengths` (in drawing units): { steps: [{ delay, duration }], fillDelay,
 * fillDuration }. Every step ends by `total`; each gets at least `least` ms (or an even share when
 * there are too many paths for that).
 */
export function strokePlan(lengths, total = TOTAL_MS, least = 80) {
  const n = lengths.length
  if (!n) return { steps: [], fillDelay: 0, fillDuration: FILL_MS }
  const floor = Math.min(least, total / n)
  const sizes = lengths.map((l) => (Number.isFinite(l) && l > 0 ? l : 0))
  const sum = sizes.reduce((a, b) => a + b, 0)
  const spare = total - floor * n
  const steps = []
  let at = 0
  for (let i = 0; i < n; i++) {
    const share = sum > 0 ? (spare * sizes[i]) / sum : spare / n
    const duration = floor + share
    steps.push({ delay: Math.round(at), duration: Math.max(1, Math.round(duration)) })
    at += duration
  }
  // rounding may leave the last a millisecond over; keep the promise
  const last = steps[n - 1]
  if (last.delay + last.duration > total) last.duration = Math.max(1, total - last.delay)
  return { steps, fillDelay: Math.round(Math.min(at, total)), fillDuration: FILL_MS }
}
