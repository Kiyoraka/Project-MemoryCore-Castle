// "0:07" / "12:03" for a position in seconds; anything unusable (not loaded yet, infinite) shows 0:00.
export function playerClock(seconds) {
  const s = Number.isFinite(seconds) && seconds > 0 ? Math.floor(seconds) : 0
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
}
