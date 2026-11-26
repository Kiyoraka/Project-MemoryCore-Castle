// MUSIC STUDIO's actions (taken from the Alice Keep's studio). Every control on the page is a named
// action with its values; the member's button and the companion's chat line call the SAME action
// through this registry. Each action cleans its values (check: clamp or throw a plain error), makes
// the next state (apply) and says itself in a few words (label). Paid actions are never applied by
// the page: paidWord turns them into the one line the page types into the room's chat, where the
// companion says the cost and waits for the member's yes before the song API is called.

import { lineTimes, lyricLines } from './lyrics.js'

export const STEPS = ['seed', 'draft', 'sing', 'fix', 'dress', 'finish']
export const MODELS = ['V4', 'V4_5', 'V4_5PLUS', 'V5']
export const SEED_KINDS = ['picture', 'audio', 'midi', 'video']
export const LOOP_PARTS = ['intro', 'outro']
export const VOCAL_GENDERS = ['m', 'f', 'any']
export const DIAL_NAMES = ['model', 'length', 'styleWeight', 'weirdness', 'variety', 'audioWeight', 'vocalGender', 'persona']
export const QUARTERS = ['intro', 'verse', 'chorus', 'bridge']
export const MAX_STEM_VOLUME = 1.5
export const MAX_STEMS = 12
export const EXPORT_KINDS = ['sound', 'wav', 'video', 'cover']
// the song API's own limits for references: pictures up to 5, one video
const SEED_LIMITS = { picture: 5, audio: 4, midi: 1, video: 1 }
const HISTORY_LIMIT = 60
// what an undo never rewinds: where he is, what is playing, what the room brought home
const LIVE_KEYS = ['song', 'step', 'duration', 'playing', 'at', 'credits', 'takes', 'exports', 'timing', 'timingLines']

export const DIAL_DEFAULTS = Object.freeze({
  model: 'V5',
  length: 180,
  styleWeight: 0.5,
  weirdness: 0.5,
  variety: 2,
  audioWeight: 0.5,
  vocalGender: 'f',
  persona: '',
})

/** A fresh studio: no song open, the SEED step, the dials at their middle. */
export function newState() {
  return {
    song: { album: '', track: 0, title: '' },
    step: 'seed',
    duration: 0,
    at: 0,
    playing: false,
    sections: [],
    selection: null,
    draft: { title: '', style: '', lyrics: '', negative: '' },
    dials: { ...DIAL_DEFAULTS },
    stems: [],
    takes: [],
    chosenTake: null,
    seeds: [],
    loops: { intro: '', outro: '' },
    karaoke: false,
    credits: null,
    // what her door brought home: [{ kind: sound|wav|video|cover, file }]
    exports: [],
    // the chosen take's word timing file ({ take, file }) and its line starts, once read
    timing: null,
    timingLines: [],
  }
}

// ---------- cleaning values ----------

const fail = (text) => {
  throw new Error(text)
}
const clamp = (n, lo, hi) => Math.min(hi, Math.max(lo, n))
const round2 = (n) => Math.round(n * 100) / 100

function num(value, what) {
  const n = typeof value === 'string' && value.trim() !== '' ? Number(value) : value
  if (typeof n !== 'number' || !Number.isFinite(n)) fail(`${what} must be a number`)
  return n
}

function text(value, max, what) {
  if (value == null) return ''
  if (typeof value !== 'string') fail(`${what} must be text`)
  return value.slice(0, max)
}

function bool(value, what) {
  if (typeof value === 'boolean') return value
  if (value === 'true' || value === 1) return true
  if (value === 'false' || value === 0) return false
  return fail(`${what} must be true or false`)
}

function oneOf(value, list, what) {
  return list.includes(value) ? value : fail(`${what} must be one of ${list.join(', ')}`)
}

function needed(value, what) {
  if (typeof value !== 'string' || !value.trim()) fail(`${what} is needed`)
  return value.trim()
}

const songEnd = (state) => (state?.duration > 0 ? state.duration : Infinity)

function stemNamed(state, name) {
  const stem = state.stems.find((s) => s.name === name)
  return stem ? stem.name : fail(`There is no stem named ${name}`)
}

/** A place in the song in seconds, kept inside the song. */
function place(value, state, what) {
  const end = songEnd(state)
  return round2(clamp(num(value, what), 0, Number.isFinite(end) ? end : Number.MAX_SAFE_INTEGER))
}

export const clock = (s) => (Number.isFinite(s) && s >= 0 ? `${Math.floor(s / 60)}:${String(Math.floor(s % 60)).padStart(2, '0')}` : '0:00')

// ---------- lyrics by section ----------

/** "[Verse 1]\nline\n\n[Chorus]\n..." -> [{ section, text }]; words before any label are "Lyrics". */
export function lyricSections(lyrics) {
  const out = []
  let current = null
  for (const raw of (lyrics || '').split('\n')) {
    const line = raw.trim()
    const label = /^\[(.+)\]$/.exec(line)
    if (label) {
      current = { section: label[1].trim(), text: '' }
      out.push(current)
    } else if (line) {
      if (!current) {
        current = { section: 'Lyrics', text: '' }
        out.push(current)
      }
      current.text = current.text ? `${current.text}\n${line}` : line
    }
  }
  return out
}

/** The lyrics with one section's words replaced (or the section added at the end). */
export function setSectionLyrics(lyrics, section, words) {
  const parts = lyricSections(lyrics)
  const at = parts.findIndex((p) => p.section.toLowerCase() === section.toLowerCase())
  if (at >= 0) parts[at] = { section: parts[at].section, text: words.trim() }
  else parts.push({ section, text: words.trim() })
  return parts.map((p) => (p.section === 'Lyrics' && parts.length === 1 ? p.text : `[${p.section}]\n${p.text}`)).join('\n\n')
}

/** The sung lines whose (timed or estimated) start falls in [from, to) - the words of a part. */
export function wordsBetween(lyrics, duration, from, to, timing = null) {
  const lines = lyricLines(lyrics)
  return lineTimes(lines, duration, timing)
    .filter((t) => t.start >= from && t.start < to)
    .map((t) => lines[t.index].text)
    .join('\n')
}

// ---------- sections, waveform and stems (shared by the page and her) ----------

/** Her section list when she gave one, else the song cut into even quarters. */
export function sectionsOf(state) {
  if (state.sections.length) return state.sections
  const timed = timingSections(state.draft.lyrics, state.timingLines ?? [], state.duration)
  return timed.length ? timed : defaultSections(state.duration)
}

/**
 * The song's sections from its lyric labels and the take's timed line starts: each [Label] begins
 * at its first sung line (the first at 0), ends where the next begins. [] when the timing does not
 * match the lyrics line for line, or the lyrics carry no labels.
 */
export function timingSections(lyrics, starts, duration) {
  const lines = lyricLines(lyrics)
  const sung = lines.filter((l) => l.kind === 'line').length
  if (!sung || sung !== starts.length || !(duration > 0) || !lines.some((l) => l.kind === 'label')) return []
  const marks = []
  let label = null
  let n = 0
  for (const l of lines) {
    if (l.kind === 'label') {
      label = l.text
      continue
    }
    if (label !== null || !marks.length) marks.push({ name: label ?? 'Lyrics', from: marks.length ? starts[n] : 0 })
    label = null
    n += 1
  }
  return marks
    .map((m, i) => ({ name: m.name, from: round2(m.from), to: round2(i + 1 < marks.length ? marks[i + 1].from : duration) }))
    .filter((m) => m.to - m.from >= 1)
}

/**
 * A take's timing file, either shape: the new { lines: [{ start, text }], words: [{ word, start, end }] }
 * or the old { lines: [12.4, ...] } (a bare list too). -> { lines, texts, words } or null.
 */
export function parseTiming(data) {
  if (typeof data === 'string') {
    try {
      data = JSON.parse(data.replace(/^\uFEFF/, ''))
    } catch {
      return null
    }
  }
  if (Array.isArray(data)) data = { lines: data }
  if (!data || typeof data !== 'object') return null
  const lines = []
  const texts = []
  for (const l of Array.isArray(data.lines) ? data.lines : []) {
    if (typeof l === 'number' && Number.isFinite(l)) {
      lines.push(round2(l))
      texts.push(null)
    } else if (l && typeof l === 'object' && l.start != null && Number.isFinite(Number(l.start))) {
      lines.push(round2(Number(l.start)))
      texts.push(typeof l.text === 'string' ? l.text : null)
    }
  }
  const words = (Array.isArray(data.words) ? data.words : [])
    .filter((w) => w && typeof w.word === 'string' && w.start != null && Number.isFinite(Number(w.start)))
    .map((w) => {
      const start = round2(Number(w.start))
      const end = w.end != null && Number.isFinite(Number(w.end)) ? round2(Number(w.end)) : start
      return { word: w.word, start, end: Math.max(start, end) }
    })
    .sort((a, b) => a.start - b.start)
  if (!lines.length && !words.length) return null
  return { lines, texts, words }
}

/** The timed words grouped under their lines (by the line starts), for FINISH's word-by-word glow. */
export function wordLines(timing) {
  if (!timing?.words.length) return []
  const starts = timing.lines
  if (!starts.length) return [{ start: timing.words[0].start, text: null, words: timing.words }]
  const out = starts.map((start, i) => ({ start, text: timing.texts[i] ?? null, words: [] }))
  for (const w of timing.words) {
    let at = 0
    for (let i = 0; i < starts.length; i += 1) if (starts[i] <= w.start + 0.05) at = i
    out[at].words.push(w)
  }
  return out.filter((l) => l.words.length || l.text)
}

export function defaultSections(duration) {
  if (!(duration > 0)) return []
  const q = duration / 4
  return QUARTERS.map((name, n) => ({ name, from: round2(n * q), to: round2(n === 3 ? duration : (n + 1) * q) }))
}

/** The loudest sample in each of `buckets` slices, 0..1 - the waveform's bars. */
export function peaksOf(samples, buckets) {
  const out = new Array(Math.max(0, buckets)).fill(0)
  if (!samples?.length || !buckets) return out
  const size = samples.length / buckets
  for (let b = 0; b < buckets; b += 1) {
    const start = Math.floor(b * size)
    const end = Math.max(start + 1, Math.floor((b + 1) * size))
    let peak = 0
    // a stride keeps a long song quick; the peak of every slice is still close enough to see
    const stride = Math.max(1, Math.floor((end - start) / 400))
    for (let i = start; i < end && i < samples.length; i += stride) {
      const v = Math.abs(samples[i])
      if (v > peak) peak = v
    }
    out[b] = Math.min(1, peak)
  }
  return out
}

const VOCAL = /vocal|voice|vox|sing/i
export const isVocal = (name) => VOCAL.test(name)

/** What a stem plays at: muted, soloed away, or karaoke (vocals out) is 0, else its volume. */
export function stemGain(stem, stems, karaoke = false) {
  if (karaoke && isVocal(stem.name)) return 0
  if (stems.some((s) => s.solo) && !stem.solo) return 0
  if (stem.muted) return 0
  return stem.volume
}

const EXTENSIONS = {
  picture: ['png', 'jpg', 'jpeg', 'webp', 'gif'],
  audio: ['mp3', 'wav', 'm4a', 'ogg', 'flac', 'aac'],
  midi: ['mid', 'midi'],
  video: ['mp4', 'mov', 'webm', 'mkv'],
}

/** picture | audio | midi | video by the file's ending, or null. */
export function seedKind(path) {
  const ext = String(path).split('.').pop()?.toLowerCase() ?? ''
  return SEED_KINDS.find((kind) => EXTENSIONS[kind].includes(ext)) ?? null
}

/** Windows paths in pasted text: quoted ones whole, bare ones up to the next space. */
export function pathsIn(pasted) {
  const found = []
  for (const m of String(pasted || '').matchAll(/"([A-Za-z]:[\\/][^"]+)"|([A-Za-z]:[\\/]\S+)/g)) found.push(m[1] ?? m[2])
  return [...new Set(found)]
}

// ---------- the page's actions ----------

const asked = (values) => {
  if (values == null) return {}
  if (typeof values !== 'object' || Array.isArray(values)) fail('The values must be an object')
  return values
}

function cleanSection(raw, state) {
  const from = place(raw.from, state, 'from')
  const to = place(raw.to, state, 'to')
  if (!(from < to)) fail('from must come before to')
  if (to - from < 1) fail('A section is at least one second long')
  return { from, to }
}

export const EXPORT_WORDS = { sound: 'A sound', wav: 'The WAV', video: 'The video', cover: 'The cover art' }

const mapStem = (state, name, change) => ({ ...state, stems: state.stems.map((s) => (s.name === name ? { ...s, ...change } : s)) })

/**
 * The registry. group: the step whose undo it joins (null = never undone: where he is and the
 * transport). touched: what the page lights when she did it.
 */
export const ACTIONS = {
  'step.set': {
    group: null,
    check: (v) => ({ step: oneOf(asked(v).step, STEPS, 'step') }),
    apply: (s, v) => ({ ...s, step: v.step }),
    label: (v) => `Step: ${v.step}`,
    touched: () => 'step',
  },
  'song.open': {
    group: null,
    check: (v) => {
      v = asked(v)
      const track = Math.round(num(v.track ?? 1, 'track'))
      if (track < 1) fail('track starts at 1')
      return {
        album: needed(v.album, 'album').slice(0, 120),
        track,
        title: text(v.title, 120, 'title'),
        style: text(v.style, 1000, 'style'),
        lyrics: text(v.lyrics, 5000, 'lyrics'),
        step: v.step == null ? 'fix' : oneOf(v.step, STEPS, 'step'),
      }
    },
    apply: (s, v) => ({
      ...s,
      song: { album: v.album, track: v.track, title: v.title },
      step: v.step,
      duration: 0,
      at: 0,
      playing: false,
      sections: [],
      selection: null,
      stems: [],
      takes: [],
      chosenTake: null,
      loops: { intro: '', outro: '' },
      karaoke: false,
      exports: [],
      timing: null,
      timingLines: [],
      draft: { ...s.draft, title: v.title.slice(0, 80) || s.draft.title, style: v.style || s.draft.style, lyrics: v.lyrics || s.draft.lyrics },
    }),
    label: (v) => `Opened ${v.title || v.album}, track ${v.track}`,
    touched: () => 'song',
  },
  'song.new': {
    group: null,
    check: () => ({}),
    // a blank song at SEED; the dials and the credits stay as they were
    apply: (s) => ({ ...newState(), dials: { ...s.dials }, credits: s.credits }),
    label: () => 'A new song',
    touched: () => 'song',
  },
  'section.select': {
    group: 'fix',
    check: (v, s) => cleanSection(asked(v), s),
    apply: (s, v) => ({ ...s, selection: { from: v.from, to: v.to } }),
    label: (v) => `Selected ${clock(v.from)}-${clock(v.to)}`,
    touched: () => 'selection',
  },
  'section.clear': {
    group: 'fix',
    check: () => ({}),
    apply: (s) => ({ ...s, selection: null }),
    label: () => 'Selection cleared',
    touched: () => 'selection',
  },
  'sections.set': {
    group: 'fix',
    check: (v, s) => {
      const list = asked(v).sections
      if (!Array.isArray(list)) fail('sections must be a list')
      if (list.length > 40) fail('At most 40 sections')
      return {
        sections: list
          .map((raw, n) => ({ name: text(raw?.name, 24, 'name').trim() || `part ${n + 1}`, ...cleanSection(raw ?? {}, s) }))
          .sort((a, b) => a.from - b.from),
      }
    },
    apply: (s, v) => ({ ...s, sections: v.sections }),
    label: (v) => (v.sections.length ? `${v.sections.length} sections marked` : 'Sections back to quarters'),
    touched: () => 'sections',
  },
  'dial.set': {
    group: 'draft',
    check: (v) => {
      v = asked(v)
      const name = oneOf(v.name, DIAL_NAMES, 'dial')
      let value
      if (name === 'model') value = oneOf(v.value, MODELS, 'model')
      else if (name === 'vocalGender') value = oneOf(v.value, VOCAL_GENDERS, 'vocalGender')
      else if (name === 'persona') value = text(v.value, 120, 'persona').trim()
      else if (name === 'length') value = Math.round(clamp(num(v.value, 'length'), 10, 360))
      else if (name === 'variety') value = Math.round(clamp(num(v.value, 'variety'), 0, 4))
      else value = round2(clamp(num(v.value, name), 0, 1))
      return { name, value }
    },
    apply: (s, v) => ({ ...s, dials: { ...s.dials, [v.name]: v.value } }),
    label: (v) => `${v.name} ${v.name === 'length' ? `${v.value} s` : v.value === '' ? '(none)' : v.value}`,
    touched: (v) => `dial:${v.name}`,
  },
  'draft.set': {
    group: 'draft',
    check: (v) => {
      v = asked(v)
      const limits = { title: 80, style: 1000, lyrics: 5000, negative: 1000 }
      const out = {}
      for (const key of Object.keys(limits)) if (v[key] !== undefined) out[key] = text(v[key], limits[key], key)
      if (!Object.keys(out).length) fail('Give title, style, lyrics or negative')
      return out
    },
    apply: (s, v) => ({ ...s, draft: { ...s.draft, ...v } }),
    label: (v) => `Draft: ${Object.keys(v).join(', ')}`,
    touched: () => 'draft',
  },
  'lyrics.set': {
    group: 'draft',
    check: (v) => {
      v = asked(v)
      const section = needed(v.section, 'section').replace(/^\[|\]$/g, '').slice(0, 40)
      return { section, text: text(v.text, 5000, 'text') }
    },
    apply: (s, v) => ({ ...s, draft: { ...s.draft, lyrics: setSectionLyrics(s.draft.lyrics, v.section, v.text).slice(0, 5000) } }),
    label: (v) => `Lyrics of ${v.section}`,
    touched: () => 'draft',
  },
  'take.choose': {
    group: 'sing',
    check: (v, s) => {
      const take = Math.round(num(asked(v).take, 'take'))
      if (!s.takes.some((k) => k.take === take)) fail(`There is no take ${take}`)
      return { take }
    },
    // the length follows when the page has read the take
    apply: (s, v) => ({ ...s, chosenTake: v.take, playing: false, at: 0 }),
    label: (v) => `Take v${v.take}`,
    touched: () => 'take',
  },
  'stem.mute': {
    group: 'dress',
    check: (v, s) => ({ stem: stemNamed(s, asked(v).stem), on: bool(v.on ?? true, 'on') }),
    apply: (s, v) => mapStem(s, v.stem, { muted: v.on }),
    label: (v) => `${v.stem} ${v.on ? 'muted' : 'unmuted'}`,
    touched: (v) => `stem:${v.stem}`,
  },
  'stem.volume': {
    group: 'dress',
    check: (v, s) => ({ stem: stemNamed(s, asked(v).stem), value: round2(clamp(num(v.value, 'value'), 0, MAX_STEM_VOLUME)) }),
    apply: (s, v) => mapStem(s, v.stem, { volume: v.value }),
    label: (v) => `${v.stem} at ${Math.round(v.value * 100)}%`,
    touched: (v) => `stem:${v.stem}`,
  },
  'stem.solo': {
    group: 'dress',
    check: (v, s) => ({ stem: stemNamed(s, asked(v).stem), on: bool(v.on ?? true, 'on') }),
    apply: (s, v) => mapStem(s, v.stem, { solo: v.on }),
    label: (v) => `${v.stem} ${v.on ? 'solo' : 'out of solo'}`,
    touched: (v) => `stem:${v.stem}`,
  },
  'stems.set': {
    group: 'dress',
    check: (v) => {
      const list = asked(v).stems
      if (!Array.isArray(list)) fail('stems must be a list')
      if (list.length > MAX_STEMS) fail(`At most ${MAX_STEMS} stems`)
      const names = new Set()
      return {
        stems: list.map((raw) => {
          const name = needed(raw?.name, 'name').slice(0, 40)
          if (names.has(name)) fail(`Two stems are named ${name}`)
          names.add(name)
          return { name, path: needed(raw?.path ?? raw?.file, 'file') }
        }),
      }
    },
    apply: (s, v) => ({
      ...s,
      stems: v.stems.map((n) => {
        const old = s.stems.find((o) => o.name === n.name)
        return { name: n.name, path: n.path, muted: old?.muted ?? false, volume: old?.volume ?? 1, solo: old?.solo ?? false }
      }),
    }),
    label: (v) => `${v.stems.length} stems on their lanes`,
    touched: () => 'stems',
  },
  'loop.lay': {
    group: 'dress',
    check: (v) => ({ part: oneOf(asked(v).part, LOOP_PARTS, 'part'), file: text(v.file, 400, 'file').trim() }),
    apply: (s, v) => ({ ...s, loops: { ...s.loops, [v.part]: v.file } }),
    label: (v) => (v.file ? `Loop laid under the ${v.part}` : `Loop taken from the ${v.part}`),
    touched: (v) => `loop:${v.part}`,
  },
  'karaoke.set': {
    group: 'finish',
    check: (v) => ({ on: bool(asked(v).on ?? true, 'on') }),
    apply: (s, v) => ({ ...s, karaoke: v.on }),
    label: (v) => `Karaoke ${v.on ? 'on' : 'off'}`,
    touched: () => 'karaoke',
  },
  // ---------- results her door brings home after a paid call (never undone: they are files) ----------
  'take.add': {
    group: null,
    check: (v) => {
      v = asked(v)
      const take = Math.round(num(v.take, 'take'))
      if (take < 1) fail('take starts at 1')
      return { take, file: needed(v.file, 'file').slice(0, 400), audioId: text(v.audioId, 120, 'audioId').trim() }
    },
    apply: (s, v) => {
      const old = s.takes.find((k) => k.take === v.take)
      const entry = {
        take: v.take,
        path: v.file,
        approved: old?.approved ?? false,
        audioId: v.audioId || old?.audioId || '',
        duration: old?.path === v.file ? old.duration ?? 0 : 0,
        added: true,
      }
      const takes = [...s.takes.filter((k) => k.take !== v.take), entry].sort((a, b) => a.take - b.take)
      return { ...s, takes, chosenTake: v.take, playing: false, at: 0 }
    },
    label: (v) => `Take v${v.take} came home`,
    touched: () => 'take',
  },
  'timing.set': {
    group: null,
    check: (v, s) => {
      const file = needed(asked(v).file, 'file').slice(0, 400)
      if (!/\.json$/i.test(file)) fail('A timing file ends in .json')
      return { file, take: s.chosenTake }
    },
    apply: (s, v) => ({ ...s, timing: { take: v.take, file: v.file } }),
    label: () => 'Word timing came home',
    touched: () => 'timing',
  },
  'export.add': {
    group: null,
    check: (v) => ({ kind: oneOf(asked(v).kind, EXPORT_KINDS, 'kind'), file: needed(v.file, 'file').slice(0, 400) }),
    apply: (s, v) => ({ ...s, exports: [...s.exports.filter((x) => x.file !== v.file), { kind: v.kind, file: v.file }] }),
    label: (v) => `${EXPORT_WORDS[v.kind]} came home`,
    touched: (v) => `export:${v.kind}`,
  },
  'credits.set': {
    group: null,
    check: (v) => {
      const value = num(asked(v).value, 'value')
      if (value < 0) fail('credits are never below 0')
      return { value: round2(value) }
    },
    apply: (s, v) => ({ ...s, credits: v.value }),
    label: (v) => `${v.value} credits left`,
    touched: () => 'credits',
  },
  play: {
    group: null,
    check: (v, s) => {
      v = asked(v)
      return v.at == null ? {} : { at: place(v.at, s, 'at') }
    },
    apply: (s, v) => ({ ...s, playing: true, at: v.at ?? s.at }),
    label: (v) => (v.at == null ? 'Play' : `Play from ${clock(v.at)}`),
    touched: () => 'transport',
  },
  pause: {
    group: null,
    check: () => ({}),
    apply: (s) => ({ ...s, playing: false }),
    label: () => 'Pause',
    touched: () => 'transport',
  },
  seek: {
    group: null,
    check: (v, s) => ({ at: place(asked(v).at, s, 'at') }),
    apply: (s, v) => ({ ...s, at: v.at }),
    label: (v) => `To ${clock(v.at)}`,
    touched: () => 'transport',
  },
  'seed.add': {
    group: 'seed',
    check: (v, s) => {
      v = asked(v)
      const path = needed(v.path, 'path').slice(0, 400)
      const kind = v.kind == null ? seedKind(path) ?? fail('That file is not a picture, audio, MIDI or video') : oneOf(v.kind, SEED_KINDS, 'kind')
      if (!s.seeds.some((x) => x.path === path) && s.seeds.filter((x) => x.kind === kind).length >= SEED_LIMITS[kind]) {
        fail(`At most ${SEED_LIMITS[kind]} ${kind}${SEED_LIMITS[kind] > 1 ? 's' : ''}`)
      }
      return { kind, path }
    },
    apply: (s, v) => (s.seeds.some((x) => x.path === v.path) ? s : { ...s, seeds: [...s.seeds, { kind: v.kind, path: v.path }] }),
    label: (v) => `Seed: ${v.kind} ${v.path.split(/[\\/]/).pop()}`,
    touched: () => 'seeds',
  },
  'seed.remove': {
    group: 'seed',
    check: (v, s) => {
      const path = needed(asked(v).path, 'path')
      return s.seeds.some((x) => x.path === path) ? { path } : fail('That seed is not in the list')
    },
    apply: (s, v) => ({ ...s, seeds: s.seeds.filter((x) => x.path !== v.path) }),
    label: (v) => `Seed removed: ${v.path.split(/[\\/]/).pop()}`,
    touched: () => 'seeds',
  },
}

/**
 * One action on the state: -> { state, values, label, touched, group }. Throws a plain error for a
 * paid name, an unknown name or values that cannot be cleaned.
 */
export function runAction(state, name, values) {
  if (isPaid(name)) fail('paid actions go through the room')
  const action = ACTIONS[name]
  if (!action) fail(`There is no action named ${name}`)
  const clean = action.check(values, state)
  return { state: action.apply(state, clean), values: clean, label: action.label(clean), touched: action.touched(clean), group: action.group }
}

// ---------- undo: one entry per run of changes in a step, his or hers ----------

/**
 * The history after an action. Changes in a row in the same step group by the same hand fold into
 * one entry (the state before the first of them), so turning a dial ten times is one undo.
 */
export function record(history, { group, by, label, before }) {
  if (!group) return history
  const last = history[history.length - 1]
  if (last && last.group === group && last.by === by) return [...history.slice(0, -1), { ...last, label }]
  return [...history, { group, by, label, before }].slice(-HISTORY_LIMIT)
}

/** Undoes the newest entry -> { state, history, entry }, or null when there is nothing to undo. */
export function undoLast(history, state) {
  const entry = history[history.length - 1]
  if (!entry) return null
  const restored = { ...entry.before }
  for (const key of LIVE_KEYS) restored[key] = state[key]
  // a chosen take that is no longer on the shelf stays as it is now
  if (!restored.takes.some((k) => k.take === restored.chosenTake)) restored.chosenTake = state.chosenTake
  return { state: restored, history: history.slice(0, -1), entry }
}

// ---------- live facts the page sets (never actions, never undone) ----------

export const withDuration = (state, duration) => (state.duration === duration ? state : { ...state, duration: Number.isFinite(duration) ? round2(duration) : 0 })
export const withCredits = (state, credits) => ({ ...state, credits })
export const withAt = (state, at, playing = state.playing) => ({ ...state, at: round2(Math.max(0, at)), playing })

/**
 * The takes the shelf holds for the open song, with the takes her door added that the shelf does
 * not show yet; a take keeps its audioId and its length. The chosen one stays, else the approved,
 * else the newest.
 */
export function withTakes(state, takes) {
  const shelf = takes.map((k) => {
    const old = state.takes.find((o) => o.take === k.take)
    return {
      take: k.take,
      path: k.path,
      approved: !!k.approved,
      audioId: old?.audioId ?? '',
      duration: old && old.path === k.path ? old.duration ?? 0 : 0,
      added: false,
    }
  })
  const extra = state.takes.filter((o) => o.added && !shelf.some((k) => k.take === o.take))
  const list = [...shelf, ...extra].sort((a, b) => a.take - b.take)
  let chosen = state.chosenTake
  if (!list.some((k) => k.take === chosen)) chosen = (list.find((k) => k.approved) ?? list[list.length - 1])?.take ?? null
  if (chosen === state.chosenTake && JSON.stringify(list) === JSON.stringify(state.takes)) return state
  return { ...state, takes: list, chosenTake: chosen }
}

/** The length of one take, once the page has read it. */
export function withTakeDuration(state, take, duration) {
  const d = Number.isFinite(duration) ? round2(duration) : 0
  if (!state.takes.some((k) => k.take === take && k.duration !== d)) return state
  return { ...state, takes: state.takes.map((k) => (k.take === take ? { ...k, duration: d } : k)) }
}

/** The chosen take's timing, read: its line starts (or none). */
export const withTiming = (state, parsed) => ({ ...state, timingLines: parsed?.lines ?? [] })

// ---------- paid actions: words for her room ----------

function cleanSing(v) {
  v = asked(v)
  const out = {
    title: text(v.title, 80, 'title').trim(),
    style: text(v.style, 1000, 'style').trim(),
    lyrics: text(v.lyrics, 5000, 'lyrics').trim(),
    negative: text(v.negative, 1000, 'negative').trim(),
  }
  if (!out.lyrics && !out.style) fail('Write the lyrics or a style first')
  for (const name of DIAL_NAMES) if (v[name] !== undefined) out[name] = ACTIONS['dial.set'].check({ name, value: v[name] }).value
  if (v.seeds !== undefined) {
    if (!Array.isArray(v.seeds)) fail('seeds must be a list')
    out.seeds = v.seeds.slice(0, 12).map((x) => ({ kind: oneOf(x?.kind, SEED_KINDS, 'kind'), path: needed(x?.path, 'path') }))
  }
  return out
}

const noValues = () => ({})

/** The paid actions: check (clean or throw) and label. The page never applies them. */
export const PAID = Object.freeze({
  sing: { check: cleanSing, label: () => 'Sing two takes' },
  resing: {
    check: (v, s) => {
      v = asked(v)
      const from = round2(Math.max(0, num(v.from, 'from')))
      const to = round2(num(v.to, 'to'))
      if (!(from < to)) fail('from must come before to')
      if (to - from < 10) fail('A re-sung part is at least 10 seconds')
      const end = songEnd(s)
      if (Number.isFinite(end)) {
        if (to > end) fail('The part ends after the song')
        if (to - from > end / 2) fail('A re-sung part is at most half the song')
      }
      return { from, to, lyrics: needed(v.lyrics, 'lyrics').slice(0, 3000) }
    },
    label: (v) => `Re-sing ${clock(v.from)}-${clock(v.to)}`,
  },
  extend: {
    check: (v, s) => {
      const at = round2(Math.max(0, num(asked(v).at, 'at')))
      if (at >= songEnd(s)) fail('Extend from a place inside the song')
      return { at }
    },
    label: (v) => `Extend from ${clock(v.at)}`,
  },
  cover: { check: (v) => ({ style: needed(asked(v).style, 'style').slice(0, 1000) }), label: (v) => `Cover in ${v.style}` },
  stems: {
    check: (v) => {
      const mode = Number(asked(v).mode)
      return mode === 2 || mode === 12 ? { mode } : fail('mode must be 2 or 12')
    },
    label: (v) => `Split into ${v.mode === 2 ? '2' : 'up to 12'} stems`,
  },
  sound: {
    check: (v) => {
      v = asked(v)
      const out = { prompt: needed(v.prompt, 'prompt').slice(0, 500) }
      const key = text(v.key, 24, 'key').trim()
      if (key) {
        if (!/^[A-Ga-g][#b♯♭]?\s*(m|maj|min|major|minor)?$/.test(key)) fail('key looks like C, F# minor or Bb major')
        out.key = key
      }
      if (v.tempo != null && v.tempo !== '') out.tempo = Math.round(clamp(num(v.tempo, 'tempo'), 1, 300))
      out.loop = v.loop == null ? true : bool(v.loop, 'loop')
      return out
    },
    label: (v) => `Sound: ${v.prompt.slice(0, 40)}`,
  },
  timing: { check: noValues, label: () => 'Word timing' },
  wav: { check: noValues, label: () => 'WAV' },
  cover_art: { check: noValues, label: () => 'Cover art' },
  video: { check: noValues, label: () => 'Music video' },
  style_expand: { check: (v) => ({ style: needed(asked(v).style, 'style').slice(0, 200) }), label: (v) => `Expand style: ${v.style}` },
})

export const isPaid = (name) => Object.prototype.hasOwnProperty.call(PAID, name)

/**
 * The one line the page types into the Music room for a paid action, e.g.
 * `studio resing {"from":42,"to":61,"lyrics":"...","album":"x","track":1,"take":2}` - compact JSON
 * on one line. With the state, the open song (album, track, chosen take) rides along.
 */
export function paidWord(name, values, state = null) {
  if (!isPaid(name)) fail(`${name} is not a paid action`)
  const clean = PAID[name].check(values, state)
  if (state?.song?.album && name !== 'style_expand') {
    clean.album ??= state.song.album
    clean.track ??= state.song.track
    if (state.chosenTake != null) clean.take ??= state.chosenTake
  }
  // one line, always: JSON escapes every new line inside the lyrics
  return `studio ${name} ${JSON.stringify(clean)}`
}

// ---------- what she reads (state.json) ----------

/** The selection with the section it sits in and the words sung under it, when known. */
export function selectionForHer(state) {
  const { from, to } = state.selection
  const out = { from, to }
  let best = null
  let most = 0
  for (const s of sectionsOf(state)) {
    const overlap = Math.min(to, s.to) - Math.max(from, s.from)
    if (overlap > most) {
      most = overlap
      best = s
    }
  }
  if (best && most >= (to - from) / 2) out.section = best.name
  const words = wordsBetween(state.draft.lyrics, state.duration, from, to, state.timingLines?.length ? state.timingLines : null)
  if (words) out.lyrics = words
  return out
}

/** The page as she reads it: the open song, the step, the sections, lyrics, dials, stems, takes... */
export function stateForHer(state) {
  return {
    song: { ...state.song },
    step: state.step,
    duration: state.duration,
    at: state.at,
    playing: state.playing,
    sections: sectionsOf(state).map((s) => ({ name: s.name, from: s.from, to: s.to })),
    selection: state.selection ? selectionForHer(state) : null,
    draft: { ...state.draft },
    lyrics: lyricSections(state.draft.lyrics),
    dials: { ...state.dials },
    stems: state.stems.map((s) => ({ name: s.name, muted: s.muted, volume: s.volume, solo: s.solo })),
    takes: state.takes.map((k) => ({ take: k.take, file: k.path, audioId: k.audioId ?? '', duration: k.duration ?? 0, approved: k.approved })),
    chosenTake: state.chosenTake,
    seeds: state.seeds.map((x) => ({ ...x })),
    loops: { ...state.loops },
    karaoke: state.karaoke,
    credits: state.credits,
    exports: state.exports.map((x) => ({ ...x })),
    timing: state.timing ? { ...state.timing } : null,
    actions: Object.keys(ACTIONS),
    paid: Object.keys(PAID),
  }
}
