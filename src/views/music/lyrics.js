// MUSIC STUDIO's lyrics, ready for the timeline and FINISH, and the line probably being sung.
// Without a timing file the glow is an estimate: a short quiet intro, then the sung lines share the
// song by their length (a long line takes longer); section labels take no time.

const INTRO = 0.06 // the share of the song before the first line
const OUTRO = 0.05 // and after the last

// "[Verse 1]\nline\nline\n\n[Chorus]\n..." -> [{ kind: 'label'|'line', text }]
export function lyricLines(text) {
  const out = []
  for (const raw of (text || '').split('\n')) {
    const line = raw.trim()
    if (!line) continue
    if (/^\[.*\]$/.test(line)) out.push({ kind: 'label', text: line.slice(1, -1) })
    else out.push({ kind: 'line', text: line })
  }
  return out
}

// When each sung line starts, in seconds: [{ index, start }] for the 'line' entries. A take timed to
// the voice (`timing`: the start of the nth sung line, labels not counted) is followed as it is;
// without one, or when its count no longer matches the lyrics, the glow is the estimate above.
export function lineTimes(lines, duration, timing = null) {
  const sung = lines.map((l, index) => ({ ...l, index })).filter((l) => l.kind === 'line')
  if (sung.length && Array.isArray(timing) && timing.length === sung.length) {
    return sung.map((l, n) => ({ index: l.index, start: timing[n] }))
  }
  if (!sung.length || !(duration > 0)) return []
  const weight = (l) => Math.max(4, l.text.length)
  const total = sung.reduce((sum, l) => sum + weight(l), 0)
  const span = duration * (1 - INTRO - OUTRO)
  let at = duration * INTRO
  return sung.map((l) => {
    const start = at
    at += (span * weight(l)) / total
    return { index: l.index, start }
  })
}

// The index (into lines) of the line sung at `time`, or -1 before the first.
export function lineAt(times, time) {
  let found = -1
  for (const t of times) {
    if (t.start <= time) found = t.index
    else break
  }
  return found
}
