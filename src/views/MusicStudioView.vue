<script setup>
// MUSIC STUDIO (taken from the Alice Keep's studio, the console the member chose): six steps from a
// seed to a finished song, one large timeline with its stems, the song API's dials, the credits, and
// the room's own session as the CHAT column. Every control is a named action from musicActions.js;
// the member's hand and the companion's chat line run the same one, the companion's changes light up
// as theirs, and each step keeps its own undo. Paid steps are never run here: the page types their
// word into the room's chat, where the companion says the cost and waits for the member's yes.
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import TerminalPanel from '../components/TerminalPanel.vue'
import GlassSelect from '../components/GlassSelect.vue'
import {
  creditsRead,
  musicActionDone,
  musicAlbums,
  musicApprove,
  musicClose,
  musicPrepare,
  musicSeedCopy,
  musicStateWrite,
  onMusicAction,
  sendMessage,
} from '../api/castle.js'
import { monthNow } from '../components/widgets/credits.js'
import { lineAt, lineTimes, lyricLines } from './music/lyrics.js'
import {
  ACTIONS,
  EXPORT_WORDS,
  LOOP_PARTS,
  PAID,
  clock,
  isPaid,
  newState,
  paidWord,
  parseTiming,
  peaksOf,
  record,
  runAction,
  sectionsOf,
  seedKind,
  stateForHer,
  stemGain,
  undoLast,
  withAt,
  withCredits,
  withDuration,
  withTakeDuration,
  withTakes,
  withTiming,
  wordLines,
  wordsBetween,
} from './music/musicActions.js'
import { createEngine } from './music/studioAudio.js'
import StepRail from './music/StepRail.vue'
import Timeline from './music/Timeline.vue'
import StemLanes from './music/StemLanes.vue'
import DialBar from './music/DialBar.vue'
import SeedZone from './music/SeedZone.vue'

const props = defineProps({
  companion: { type: String, default: 'Your companion' },
  cli: { type: String, default: 'claude' },
  visible: { type: Boolean, default: true },
})
const emit = defineEmits(['keys'])

const SHELF_EVERY = 6000
const CREDITS_EVERY = 60000

const ready = ref(false)
const stamp = ref(0)
const albums = ref([])
const state = ref(newState())
const history = ref([])
const touched = ref('')
const notice = ref('')
const bridgeProblem = ref('')
const root = ref(null)
const chatBox = ref(null)
const dropping = ref(false)
const playhead = ref(0)
const takePeaks = ref([])
// the chosen take's timing file, read: { lines, texts, words } or null
const timing = ref(null)
const stemPeaks = ref({})
const timelineNote = ref('')
const form = ref(null) // a paid action's little form: { name, ...fields }
let touchTimer
let noticeTimer
let writeTimer
let creditTimer
let shelfTimer
let stopHer
let stopDrop
let frame = 0
let takeBuffer = null
let takeBufferPath = ''
const stemBuffers = new Map() // path -> AudioBuffer
const loopBuffers = new Map()

const engine = createEngine({
  onEnd: () => {
    playhead.value = 0
    state.value = withAt(state.value, 0, false)
  },
})

function say(text) {
  notice.value = text
  clearTimeout(noticeTimer)
  noticeTimer = setTimeout(() => (notice.value = ''), 5000)
}

function flash(what) {
  touched.value = what
  clearTimeout(touchTimer)
  touchTimer = setTimeout(() => (touched.value = ''), 1700)
}

const fileName = (path) => String(path).split(/[\\/]/).pop()

// ---------- the room and the shelf ----------

async function prepare() {
  try {
    await musicPrepare()
    ready.value = true
    stamp.value += 1
    bridgeProblem.value = ''
  } catch (reason) {
    bridgeProblem.value = String(reason)
  }
  await loadAlbums()
}

async function loadAlbums() {
  try {
    albums.value = await musicAlbums()
  } catch {
    // the shelf stays as it was; read again soon
  }
}

const album = computed(() => albums.value.find((a) => a.folder === state.value.song.album) ?? null)
const track = computed(() => album.value?.tracks.find((t) => t.number === state.value.song.track) ?? null)
const take = computed(() => state.value.takes.find((k) => k.take === state.value.chosenTake) ?? null)
const sections = computed(() => sectionsOf(state.value))

function syncTakes() {
  const t = track.value
  state.value = withTakes(state.value, (t?.takes ?? []).map((k) => ({ take: k.take, path: k.path, approved: (t?.approved ?? []).includes(k.take) })))
  // the same take as before (the song opened again): its length is known already
  if (takeBuffer && take.value?.path === takeBufferPath) state.value = withDuration(state.value, takeBuffer.duration)
}
watch(track, syncTakes, { immediate: true })

// ---------- actions: the member's hand and the companion's ----------

/** song.open by name alone (as the companion may send it): the shelf fills in the title, style and words. */
function enrich(name, values) {
  if (name !== 'song.open' || !values || typeof values !== 'object') return values
  const a = albums.value.find((x) => x.folder === values.album)
  if (!a) throw new Error(`There is no album ${values.album} on the shelf`)
  const t = values.track == null ? a.tracks[0] : a.tracks.find((x) => x.number === Number(values.track))
  if (!t) throw new Error(`${a.title || a.folder} has no track ${values.track}`)
  return { title: t.title || a.title, style: t.style ?? '', lyrics: t.lyrics ?? '', ...values, track: t.number }
}

function perform(name, values, by) {
  const before = state.value
  const out = runAction(before, name, enrich(name, values))
  state.value = out.state
  history.value = name === 'song.open' || name === 'song.new' ? [] : record(history.value, { group: out.group, by, label: out.label, before })
  effect(name, out.values)
  if (by === 'companion') flash(out.touched)
  return out
}

/** The member's hand: the same action, its refusal said on the page. */
function act(name, values) {
  try {
    perform(name, values, 'member')
  } catch (reason) {
    say(reason.message ?? String(reason))
  }
}

function undo() {
  const back = undoLast(history.value, state.value)
  if (!back) return false
  state.value = back.state
  history.value = back.history
  applyGains()
  restartIfPlaying()
  say(`Undid ${back.entry.label}${back.entry.by === 'companion' ? ` (${props.companion}'s change)` : ''}`)
  return true
}
const lastUndo = computed(() => history.value[history.value.length - 1] ?? null)

// The companion's actions: checked and applied as the member's are, lit as theirs, answered with ok and a note.
async function onCompanion(event) {
  const { id, name, values } = event ?? {}
  const done = (ok, note) => musicActionDone({ id, ok, note }).catch(() => {})
  if (isPaid(name)) return done(false, 'paid actions go through the room')
  if (name === 'undo') {
    const ok = undo()
    return done(ok, ok ? 'undone' : 'nothing to undo')
  }
  // a song the companion just sang may not be on the shelf yet
  if (name === 'song.open' && !albums.value.some((a) => a.folder === values?.album)) await loadAlbums()
  try {
    const out = perform(name, values, 'companion')
    if (name === 'play' && !state.value.playing) return done(false, 'nothing to play yet')
    done(true, out.label)
  } catch (reason) {
    done(false, reason.message ?? String(reason))
  }
  if (name === 'take.add' || name === 'export.add') loadAlbums()
}

// ---------- paid actions: a word into the room's chat ----------

async function askPaid(name, values) {
  let line
  let label
  try {
    line = paidWord(name, values, state.value)
    label = PAID[name].label(PAID[name].check(values, state.value))
  } catch (reason) {
    say(reason.message ?? String(reason))
    return false
  }
  try {
    await sendMessage(line, 'music')
  } catch {
    say(`${props.companion} is resting. Press Wake in the chat, then try again.`)
    return false
  }
  say(`Asked ${props.companion}: ${label}. The cost comes first, then your yes.`)
  return true
}

const singValues = () => ({ ...state.value.draft, ...state.value.dials, seeds: state.value.seeds })

function openForm(name) {
  const s = state.value
  if (name === 'resing') {
    if (!s.selection) return say('Select the part on the timeline first')
    const words = wordsBetween(s.draft.lyrics || track.value?.lyrics || '', s.duration, s.selection.from, s.selection.to, s.timingLines.length ? s.timingLines : null)
    form.value = { name, from: s.selection.from, to: s.selection.to, lyrics: words }
  } else if (name === 'cover') form.value = { name, style: '' }
  else if (name === 'sound') form.value = { name, prompt: '', key: '', tempo: '', loop: true }
  else if (name === 'style_expand') form.value = { name, style: s.draft.style.slice(0, 200) }
}

async function sendForm() {
  const { name, ...values } = form.value
  if (await askPaid(name, values)) form.value = null
}

// ---------- the sound ----------

function loopsNow() {
  const parts = sections.value
  if (!parts.length) return []
  const at = { intro: parts[0], outro: parts[parts.length - 1] }
  return LOOP_PARTS.filter((p) => state.value.loops[p] && loopBuffers.get(state.value.loops[p])).map((p) => ({
    name: `loop-${p}`,
    buffer: loopBuffers.get(state.value.loops[p]),
    from: at[p].from,
    to: at[p].to,
  }))
}

const stemsReady = () => state.value.stems.length > 0 && state.value.stems.every((s) => stemBuffers.has(s.path))

function startPlayback(at) {
  const s = state.value
  const tracks = stemsReady()
    ? s.stems.map((st) => ({ name: st.name, buffer: stemBuffers.get(st.path), gain: stemGain(st, s.stems, s.karaoke) }))
    : takeBuffer
      ? [{ name: 'take', buffer: takeBuffer, gain: 1 }]
      : []
  if (!engine.play(tracks, at, loopsNow())) {
    state.value = withAt(state.value, at, false)
    say(take.value ? 'The song is still loading' : 'Nothing to play yet - the takes come after SING')
    return
  }
  playhead.value = at
  cancelAnimationFrame(frame)
  const tick = () => {
    if (!engine.isPlaying()) return
    playhead.value = engine.position()
    frame = requestAnimationFrame(tick)
  }
  frame = requestAnimationFrame(tick)
}

function applyGains() {
  for (const st of state.value.stems) engine.setGain(st.name, stemGain(st, state.value.stems, state.value.karaoke))
}

function restartIfPlaying() {
  if (engine.isPlaying()) startPlayback(engine.position())
}

function stopPlayback() {
  const at = engine.pause()
  cancelAnimationFrame(frame)
  playhead.value = at
  return at
}

/** What an action does beyond the state: the sound follows. */
function effect(name, values) {
  if (name === 'play') startPlayback(values.at ?? playhead.value)
  else if (name === 'pause') state.value = withAt(state.value, stopPlayback(), false)
  else if (name === 'seek') {
    playhead.value = values.at
    if (engine.isPlaying()) startPlayback(values.at)
    else engine.setOffset(values.at)
  } else if (name.startsWith('stem.') || name === 'karaoke.set') {
    applyGains()
    if (name === 'karaoke.set' && values.on && !stemsReady()) say('Karaoke needs the stems - split them in DRESS')
  } else if (name === 'take.choose' || name === 'take.add' || name === 'song.open' || name === 'song.new') {
    stopPlayback()
    playhead.value = 0
    engine.setOffset(0)
    if (name === 'song.open' || name === 'song.new') syncTakes()
  } else if (name === 'timing.set') loadTiming(values.file, false)
  else if (name === 'export.add' && values.kind === 'sound' && state.value.step !== 'finish') {
    say('A sound came home - lay it under the intro or the outro in DRESS')
  } else if (name === 'loop.lay' || name === 'stems.set') restartIfPlaying()
}

function togglePlay() {
  if (state.value.playing && engine.isPlaying()) act('pause')
  else act('play', { at: playhead.value })
}

// the chosen take: decoded once for its waveform, its length and its sound
watch(
  () => take.value?.path ?? '',
  async (path) => {
    takeBuffer = null
    takeBufferPath = ''
    takePeaks.value = []
    timelineNote.value = path ? 'Reading the take...' : ''
    timing.value = null
    state.value = withTiming(state.value, null)
    if (!path) return
    // its word timing beside it, when there is one
    loadTiming(path.replace(/\.[^.\\/]+$/, '.timing.json'), true)
    try {
      const buffer = await engine.load(path)
      if ((take.value?.path ?? '') !== path) return
      takeBuffer = buffer
      takeBufferPath = path
      takePeaks.value = peaksOf(buffer.getChannelData(0), 1600)
      state.value = withTakeDuration(withDuration(state.value, buffer.duration), take.value.take, buffer.duration)
      timelineNote.value = ''
    } catch (reason) {
      timelineNote.value = `The take could not be read: ${reason.message ?? reason}`
    }
  },
  { immediate: true },
)

// the stems the companion brought home, each decoded for its lane
watch(
  () => state.value.stems.map((s) => s.path).join('|'),
  async () => {
    const want = state.value.stems
    const peaks = {}
    await Promise.all(
      want.map(async (st) => {
        try {
          const buffer = stemBuffers.get(st.path) ?? (await engine.load(st.path))
          stemBuffers.set(st.path, buffer)
          peaks[st.name] = peaksOf(buffer.getChannelData(0), 220)
        } catch {
          say(`The ${st.name} stem could not be read`)
        }
      }),
    )
    stemPeaks.value = peaks
  },
)

watch(
  () => LOOP_PARTS.map((p) => state.value.loops[p]).join('|'),
  async () => {
    for (const p of LOOP_PARTS) {
      const file = state.value.loops[p]
      if (!file || loopBuffers.has(file)) continue
      try {
        loopBuffers.set(file, await engine.load(file))
        restartIfPlaying()
      } catch {
        say(`The ${p} loop could not be read`)
      }
    }
  },
)

// ---------- FINISH: the lyrics lighting as they are sung ----------

/**
 * Reads a take's timing file (the new { lines, words } or the old { lines: [12.4] }) through the
 * asset protocol. -> true when read. Quiet when only looking beside the take.
 */
async function loadTiming(file, quiet) {
  const forPath = take.value?.path ?? ''
  try {
    const response = await fetch(convertFileSrc(file))
    if (!response.ok) throw new Error('not found')
    const parsed = parseTiming(await response.text())
    if (!parsed) throw new Error('not a timing file')
    if ((take.value?.path ?? '') !== forPath) return false
    timing.value = parsed
    state.value = withTiming(state.value, parsed)
    return true
  } catch (reason) {
    if (!quiet) say(`The word timing could not be read (${reason.message ?? reason})`)
    return false
  }
}

const timingLineStarts = computed(() => (state.value.timingLines.length ? state.value.timingLines : null))
const lines = computed(() => lyricLines(state.value.draft.lyrics || track.value?.lyrics || ''))
const times = computed(() => lineTimes(lines.value, state.value.duration, timingLineStarts.value))
const lit = computed(() => (state.value.playing ? lineAt(times.value, playhead.value) : -1))
// word by word, when the timing file carries words
const sungLines = computed(() => wordLines(timing.value))
const litWord = computed(() => {
  if (!state.value.playing || !sungLines.value.length) return null
  const at = playhead.value
  for (const [n, l] of sungLines.value.entries()) {
    const i = l.words.findIndex((w, k) => w.start <= at && at < Math.max(w.end, l.words[k + 1]?.start ?? w.end))
    if (i >= 0) return { line: n, word: i }
  }
  return null
})
const lyricBox = ref(null)
watch(lit, (n) => n >= 0 && lyricBox.value?.querySelector(`[data-line="${n}"]`)?.scrollIntoView({ block: 'center', behavior: 'smooth' }))
watch(
  () => litWord.value?.line,
  (n) => n != null && lyricBox.value?.querySelector(`[data-sung="${n}"]`)?.scrollIntoView({ block: 'center', behavior: 'smooth' }),
)
function jumpLine(n) {
  const at = times.value.find((t) => t.index === n)
  if (at) act('play', { at: at.start })
}

/** The member's own approval of the take on the timeline (never a play, never the companion). */
async function approveTake() {
  if (!album.value || !track.value || !take.value) return
  try {
    await musicApprove(album.value.folder, track.value.slug, take.value.take)
    await loadAlbums()
    syncTakes()
    say(`Take v${take.value.take} of ${track.value.title} is approved.`)
  } catch (reason) {
    say(String(reason))
  }
}

// ---------- the shelf ----------

function openAlbum(a, number) {
  const t = number == null ? a.tracks[0] : a.tracks.find((x) => x.number === number)
  act('song.open', { album: a.folder, track: t?.number ?? 1, step: t?.takes.length ? 'fix' : 'draft' })
}

// ---------- the credits meter (the Castle's own read of the member's sunoapi.org balance) ----------

const credit = ref(null)
const creditShare = computed(() => {
  const value = state.value.credits
  const whole = credit.value?.whole
  return value != null && whole ? Math.min(100, (value / whole) * 100) : 0
})
const creditLine = computed(() => {
  if (credit.value && !credit.value.hasKey) return 'No sunoapi.org key yet'
  return credit.value?.note || 'sunoapi.org balance, read every minute'
})

async function readCredits() {
  if (!props.visible) return
  try {
    const { month, start } = monthNow()
    const all = await creditsRead(month, start)
    credit.value = (all ?? []).find((c) => c.provider === 'suno') ?? null
    if (credit.value?.value != null) state.value = withCredits(state.value, credit.value.value)
  } catch {
    // read again in a minute
  }
}

// ---------- the bridge: what the companion reads, written after every change ----------

watch(state, (s) => {
  clearTimeout(writeTimer)
  writeTimer = setTimeout(async () => {
    try {
      await musicStateWrite(stateForHer(s))
      bridgeProblem.value = ''
    } catch (reason) {
      bridgeProblem.value = `${props.companion} cannot read the page yet (${String(reason)})`
    }
  }, 300)
})

// ---------- seeds: copied into the room first, that copy is the seed ----------

/** The member's seed (dropped, pasted or typed): checked, copied into the room's seeds, then seed.add. */
async function addSeed(path) {
  try {
    // refused before copying: not a seed kind, or the kind is full
    ACTIONS['seed.add'].check({ path, kind: seedKind(path) ?? undefined }, state.value)
  } catch (reason) {
    say(reason.message ?? String(reason))
    return
  }
  let copy
  try {
    copy = await musicSeedCopy(path)
    if (!copy?.path) throw new Error('no copy came back')
  } catch (reason) {
    say(`${fileName(path)} could not be copied into the studio, so it was not added (${reason.message ?? reason})`)
    return
  }
  act('seed.add', { kind: copy.kind || seedKind(copy.path) || undefined, path: copy.path })
}

// ---------- drops: on the chat for the companion, on SEED as seeds ----------

function overChat(position) {
  const box = chatBox.value?.getBoundingClientRect()
  if (!box || !position) return false
  const ratio = window.devicePixelRatio || 1
  const x = position.x / ratio
  const y = position.y / ratio
  return x >= box.left && x <= box.right && y >= box.top && y <= box.bottom
}

const keepsDrop = (drop) => props.visible && state.value.step === 'seed' && !overChat(drop.position)
// the chat leaves a SEED drop to the page
const chatTakes = (drop) => !keepsDrop(drop)

function onDragDrop(event) {
  if (!props.visible || !root.value || root.value.offsetParent === null) return
  const drop = event.payload
  if (drop.type === 'enter' || drop.type === 'over') dropping.value = state.value.step === 'seed'
  else if (drop.type === 'leave') dropping.value = false
  else if (drop.type === 'drop') {
    dropping.value = false
    if (keepsDrop(drop)) for (const path of drop.paths ?? []) addSeed(path)
  }
}

onMounted(async () => {
  await prepare()
  readCredits()
  creditTimer = setInterval(readCredits, CREDITS_EVERY)
  shelfTimer = setInterval(() => props.visible && loadAlbums(), SHELF_EVERY)
  try {
    stopHer = await onMusicAction(onCompanion)
  } catch (reason) {
    bridgeProblem.value = `${props.companion}'s hand on the page is not wired yet (${String(reason)})`
  }
  try {
    stopDrop = await getCurrentWebview().onDragDropEvent(onDragDrop)
  } catch {
    // no drop events outside Castle's window
  }
  try {
    await musicStateWrite(stateForHer(state.value))
  } catch (reason) {
    bridgeProblem.value = `${props.companion} cannot read the page yet (${String(reason)})`
  }
})

watch(
  () => props.visible,
  (on) => {
    if (!on) return
    readCredits()
    loadAlbums()
  },
)

onBeforeUnmount(() => {
  stopHer?.()
  stopDrop?.()
  clearInterval(creditTimer)
  clearInterval(shelfTimer)
  clearTimeout(writeTimer)
  clearTimeout(touchTimer)
  clearTimeout(noticeTimer)
  cancelAnimationFrame(frame)
  engine.close()
  // the room closed (its tab gone or switched off): the session ends
  musicClose().catch(() => {})
})

const audioSeeds = computed(() => [
  ...state.value.exports.filter((x) => x.kind === 'sound').map((x) => ({ path: x.file })),
  ...state.value.seeds.filter((s) => s.kind === 'audio'),
])
const sounds = computed(() => state.value.exports.filter((x) => x.kind === 'sound'))
const loopPart = ref('intro')
const loopFile = ref('')
</script>

<template>
  <div ref="root" class="studio">
    <header class="top">
      <h2><span class="mark" aria-hidden="true">✦</span> MUSIC STUDIO <span class="mark" aria-hidden="true">✦</span></h2>
      <div class="top-acts">
        <button class="ghost" :disabled="!lastUndo" :title="lastUndo ? `Undo ${lastUndo.label}` : 'Nothing to undo'" @click="undo">
          &#8630; Undo<template v-if="lastUndo && lastUndo.by === 'companion'"> ({{ companion }})</template>
        </button>
      </div>
    </header>

    <!-- the open song -->
    <section class="song card" :class="{ hers: touched === 'song' }">
      <span class="glyph" aria-hidden="true">&#9835;</span>
      <div class="song-names">
        <strong>{{ state.song.title || state.draft.title || 'A new song' }}</strong>
        <small>{{ album ? `${album.title}${track ? ` · track ${track.number}` : ''}` : 'not sung yet' }}</small>
      </div>
      <button class="ghost small" title="A blank song at SEED" aria-label="A new song" @click="act('song.new')">+</button>
    </section>

    <StepRail class="rail" :step="state.step" :hers="touched === 'step'" @set="act('step.set', { step: $event })" />

    <!-- the album shelf -->
    <aside class="shelf card" aria-label="Your albums">
      <div v-if="albums.length" class="covers">
        <button v-for="a in albums" :key="a.folder" class="cover" :class="{ on: a.folder === state.song.album }" :title="a.title" @click="openAlbum(a)">
          <img v-if="a.cover" :src="convertFileSrc(a.cover)" :alt="a.title" loading="lazy" />
          <span v-else class="blank" aria-hidden="true">&#9835;</span>
          <small>{{ a.title }}</small>
        </button>
      </div>
      <p v-else class="quiet">No albums yet. Your songs land here, and on STUDIO's Songs shelf too.</p>
      <div v-if="album && album.tracks.length > 1" class="tracks">
        <button v-for="t in album.tracks" :key="t.number" :class="{ on: t.number === state.song.track }" @click="openAlbum(album, t.number)">{{ t.number }}. {{ t.title }}</button>
      </div>
    </aside>

    <!-- the centre: the timeline, its lanes and the step's own work -->
    <main class="main card">
      <div v-if="take || state.step === 'fix' || state.step === 'dress' || state.step === 'finish'" class="transport">
        <button class="play" :aria-label="state.playing ? 'Pause' : 'Play'" :disabled="!take" @click="togglePlay">{{ state.playing ? '❚❚' : '▶' }}</button>
        <Timeline
          class="grow"
          :peaks="takePeaks"
          :duration="state.duration"
          :sections="sections"
          :selection="state.selection"
          :at="playhead"
          :action="state.step === 'fix' && state.selection ? 'Re-sing this part' : ''"
          :hers="touched === 'selection'"
          :note="take ? timelineNote : `No take yet - ${companion} sings at SING, then the song lies here.`"
          @select="act('section.select', $event)"
          @seek="act('seek', { at: $event })"
          @act="openForm('resing')"
        />
      </div>

      <StemLanes
        v-if="state.step === 'dress' || (state.stems.length && (state.step === 'fix' || state.step === 'finish'))"
        :stems="state.stems"
        :peaks="stemPeaks"
        :karaoke="state.karaoke"
        :touched="touched"
        @mute="(stem, on) => act('stem.mute', { stem, on })"
        @solo="(stem, on) => act('stem.solo', { stem, on })"
        @volume="(stem, value) => act('stem.volume', { stem, value })"
      >
        <p class="quiet">No stems yet: split the song and {{ companion }} brings them home to their lanes.</p>
      </StemLanes>

      <div class="panel">
        <!-- 1 SEED -->
        <template v-if="state.step === 'seed'">
          <SeedZone :seeds="state.seeds" :dropping="dropping" :hers="touched === 'seeds'" :companion="companion" @add="addSeed($event)" @remove="act('seed.remove', { path: $event })" />
          <div class="acts">
            <button class="violet" @click="act('step.set', { step: 'draft' })">&#10022; Shape it with {{ companion }} (DRAFT)</button>
          </div>
        </template>

        <!-- 2 DRAFT -->
        <template v-else-if="state.step === 'draft'">
          <div class="draft" :class="{ hers: touched === 'draft' }">
            <label>TITLE <input :value="state.draft.title" maxlength="80" @change="act('draft.set', { title: $event.target.value })" /></label>
            <label>STYLE <input :value="state.draft.style" maxlength="1000" placeholder="city pop, soft female vocal, rain" @change="act('draft.set', { style: $event.target.value })" /></label>
            <label>AVOID <input :value="state.draft.negative" maxlength="1000" placeholder="what the song should not be" @change="act('draft.set', { negative: $event.target.value })" /></label>
            <label class="tall">LYRICS
              <textarea :value="state.draft.lyrics" maxlength="5000" rows="8" placeholder="[Verse 1]&#10;...&#10;&#10;[Chorus]&#10;..." @change="act('draft.set', { lyrics: $event.target.value })"></textarea>
            </label>
          </div>
          <div class="acts">
            <button class="ghost" :disabled="!state.draft.style.trim()" @click="openForm('style_expand')">&#10022; Expand the style (paid)</button>
            <button class="violet" @click="askPaid('sing', singValues())">&#9835; Sing two takes (paid)</button>
          </div>
        </template>

        <!-- 3 SING -->
        <template v-else-if="state.step === 'sing'">
          <div v-if="state.takes.length" class="takes" :class="{ hers: touched === 'take' }">
            <div v-for="k in state.takes" :key="k.take" class="take" :class="{ on: k.take === state.chosenTake }">
              <button class="pick" @click="act('take.choose', { take: k.take })">{{ k.approved ? '✓ ' : '' }}Take v{{ k.take }}</button>
              <small>{{ k.take === state.chosenTake ? 'on the timeline' : fileName(k.path) }}</small>
            </div>
          </div>
          <p v-else class="quiet">The two takes land here when {{ companion }} has sung them.</p>
          <div class="acts">
            <button class="violet" @click="askPaid('sing', singValues())">&#9835; {{ state.takes.length ? 'Sing again' : 'Sing two takes' }} (paid)</button>
            <button class="ghost" :disabled="!state.takes.length" @click="act('step.set', { step: 'fix' })">On to FIX</button>
          </div>
        </template>

        <!-- 4 FIX -->
        <template v-else-if="state.step === 'fix'">
          <p class="quiet">Drag across the waveform to select a part (or click a section's name); Ctrl+wheel zooms.</p>
          <div class="acts">
            <button class="violet" :disabled="!state.selection" @click="openForm('resing')">&#8767; Re-sing this part (paid)</button>
            <button class="ghost" :disabled="!take" @click="askPaid('extend', { at: playhead })">&#10022; Extend from {{ clock(playhead) }} (paid)</button>
            <button class="ghost" :disabled="!take" @click="openForm('cover')">Cover in a style (paid)</button>
            <button class="ghost" :disabled="!state.selection" @click="act('section.clear')">Clear selection</button>
          </div>
        </template>

        <!-- 5 DRESS -->
        <template v-else-if="state.step === 'dress'">
          <div class="acts">
            <button class="violet" :disabled="!take" @click="askPaid('stems', { mode: 2 })">Split: vocals + music (paid)</button>
            <button class="ghost" :disabled="!take" @click="askPaid('stems', { mode: 12 })">Split: up to 12 stems (paid)</button>
            <button class="ghost" @click="openForm('sound')">&#10022; A sound or loop (paid)</button>
          </div>
          <div class="loops">
            <strong>LAY A LOOP</strong>
            <GlassSelect v-model="loopPart" class="part" :options="LOOP_PARTS.map((p) => ({ value: p, label: `under the ${p}` }))" aria-label="Under which part" />
            <input v-model="loopFile" list="music-studio-loop-files" placeholder="the loop's file path" />
            <datalist id="music-studio-loop-files">
              <option v-for="s in audioSeeds" :key="s.path" :value="s.path" />
            </datalist>
            <button class="ghost" :disabled="!loopFile.trim()" @click="act('loop.lay', { part: loopPart, file: loopFile })">Lay</button>
            <span v-for="p in LOOP_PARTS" :key="p" class="laid" :class="{ hers: touched === `loop:${p}` }">
              <template v-if="state.loops[p]">{{ p }}: {{ fileName(state.loops[p]) }} <button class="x" :aria-label="`Lift the ${p} loop`" @click="act('loop.lay', { part: p, file: '' })">&times;</button></template>
            </span>
          </div>
          <ul v-if="sounds.length" class="came-home" :class="{ hers: touched === 'export:sound' }">
            <li v-for="x in sounds" :key="x.file">
              <span class="kind">SOUND</span>
              <span class="file" :title="x.file">{{ fileName(x.file) }}</span>
              <button v-for="p in LOOP_PARTS" :key="p" class="ghost small-text" @click="act('loop.lay', { part: p, file: x.file })">under the {{ p }}</button>
            </li>
          </ul>
        </template>

        <!-- 6 FINISH -->
        <template v-else>
          <div class="finish">
            <div class="lyrics">
              <header>
                <label class="karaoke" :class="{ hers: touched === 'karaoke' }">
                  <input type="checkbox" :checked="state.karaoke" @change="act('karaoke.set', { on: $event.target.checked })" /> karaoke
                </label>
                <small :class="{ hers: touched === 'timing' }">{{ sungLines.length ? 'word by word' : timingLineStarts ? 'timed to the voice' : 'estimated glow' }}</small>
              </header>
              <!-- word by word, from the timing file's words (as sung) -->
              <div v-if="sungLines.length" ref="lyricBox" class="words">
                <p v-for="(l, n) in sungLines" :key="n" class="line sung" :data-sung="n" @click="act('play', { at: l.start })">
                  <span
                    v-for="(w, i) in l.words"
                    :key="i"
                    class="word"
                    :class="{ lit: litWord && litWord.line === n && litWord.word === i, past: litWord && (n < litWord.line || (n === litWord.line && i < litWord.word)) }"
                  >{{ w.word }} </span>
                </p>
              </div>
              <div v-else ref="lyricBox" class="words">
                <template v-for="(l, n) in lines" :key="n">
                  <p v-if="l.kind === 'label'" class="label">{{ l.text }}</p>
                  <p v-else class="line" :class="{ lit: n === lit }" :data-line="n" @click="jumpLine(n)">{{ l.text }}</p>
                </template>
                <p v-if="!lines.length" class="quiet">No lyrics yet.</p>
              </div>
              <ul v-if="state.exports.length" class="came-home" :class="{ hers: touched.startsWith('export:') }">
                <li v-for="x in state.exports" :key="x.file">
                  <img v-if="x.kind === 'cover'" :src="convertFileSrc(x.file)" alt="Cover art" loading="lazy" />
                  <span v-else class="kind">{{ x.kind.toUpperCase() }}</span>
                  <span class="file" :title="x.file">{{ EXPORT_WORDS[x.kind] }} &middot; {{ fileName(x.file) }}</span>
                </li>
              </ul>
            </div>
            <div class="exports">
              <button class="ghost" :disabled="!take" @click="askPaid('timing', {})">Word timing (paid)</button>
              <button class="ghost" :disabled="!take" @click="askPaid('wav', {})">WAV (paid)</button>
              <button class="ghost" :disabled="!take" @click="askPaid('cover_art', {})">Cover art (paid)</button>
              <button class="ghost" :disabled="!take" @click="askPaid('video', {})">Music video (paid)</button>
              <button class="gold" :disabled="!take || take.approved || !track" @click="approveTake">{{ take?.approved ? 'Approved' : 'Approve take' }}</button>
              <small class="quiet">Your songs are on STUDIO's Songs shelf too.</small>
            </div>
          </div>
        </template>
      </div>
      <p v-if="notice" class="notice" role="status">{{ notice }}</p>
    </main>

    <!-- CHAT: the room's own session -->
    <section ref="chatBox" class="chat card" aria-label="MUSIC STUDIO's chat" style="--panel-left: 0px; --panel-top: 0px; --panel-bottom: 0px; --panel-width: 100%">
      <TerminalPanel v-if="ready" room="music" :settings-stamp="stamp" :companion="companion" :cli="cli" :drop-filter="chatTakes" @setup="prepare" @settings="prepare" />
      <p v-else class="quiet waiting">{{ bridgeProblem || 'Opening the studio...' }}</p>
    </section>

    <DialBar
      class="dials"
      :dials="state.dials"
      :stems="state.stems"
      :touched="touched"
      @dial="(name, value) => act('dial.set', { name, value })"
      @volume="(stem, value) => act('stem.volume', { stem, value })"
    />

    <section class="credits card" :class="{ hers: touched === 'credits' }">
      <span class="coin" aria-hidden="true">&#9813;</span>
      <div class="credit-text">
        <strong>{{ state.credits != null ? `${state.credits} credits` : '...' }}</strong>
        <small>{{ creditLine }}</small>
        <button v-if="credit && !credit.hasKey" class="ghost small-text" @click="emit('keys')">Add it in Settings &gt; Keys</button>
        <div v-else class="meter" :title="credit?.whole ? `of ${credit.whole} since the last top-up` : ''"><span :style="{ width: `${creditShare}%` }"></span></div>
      </div>
    </section>

    <p v-if="bridgeProblem && ready" class="bridge" role="status">{{ bridgeProblem }}</p>

    <!-- a paid action's little form -->
    <div v-if="form" class="dialog" @click.self="form = null">
      <div class="form-card">
        <template v-if="form.name === 'resing'">
          <h3>RE-SING {{ clock(form.from) }}-{{ clock(form.to) }}</h3>
          <p>The new words for this part (10 s to half the song). {{ companion }} says the cost first.</p>
          <textarea v-model="form.lyrics" rows="6"></textarea>
        </template>
        <template v-else-if="form.name === 'cover'">
          <h3>A COVER IN ANOTHER STYLE</h3>
          <input v-model="form.style" placeholder="acoustic, piano ballad" />
        </template>
        <template v-else-if="form.name === 'sound'">
          <h3>A SOUND OR LOOP</h3>
          <input v-model="form.prompt" maxlength="500" placeholder="soft rain on a window, distant thunder" />
          <div class="row">
            <input v-model="form.key" placeholder="key (F# minor)" />
            <input v-model="form.tempo" type="number" min="1" max="300" placeholder="tempo (BPM)" />
            <label><input v-model="form.loop" type="checkbox" /> loop</label>
          </div>
        </template>
        <template v-else-if="form.name === 'style_expand'">
          <h3>EXPAND THE STYLE</h3>
          <input v-model="form.style" maxlength="200" />
        </template>
        <div class="acts">
          <button class="ghost" @click="form = null">Not now</button>
          <button class="violet" @click="sendForm">Ask {{ companion }} (paid)</button>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.studio {
  position: relative;
  display: grid;
  grid-template-columns: minmax(170px, 220px) minmax(0, 1fr) minmax(300px, 360px);
  grid-template-rows: auto auto minmax(0, 1fr) auto;
  grid-template-areas:
    'top top top'
    'song rail rail'
    'shelf main chat'
    'dials dials credits';
  gap: 12px;
  height: 100%;
  padding: 14px 18px;
  box-sizing: border-box;
  color: var(--text);
}

.top {
  grid-area: top;
  display: flex;
  align-items: center;
  gap: 12px;
}

.top h2 {
  margin: 0;
  font-weight: 500;
  font-size: 1.35rem;
  letter-spacing: 0.3em;
}

.mark {
  font-size: 1.1rem;
  color: var(--accent);
}

.top-acts {
  display: flex;
  gap: 8px;
  margin-left: auto;
}

.card {
  border: 1px solid color-mix(in srgb, var(--accent) 45%, transparent);
  border-radius: 16px;
  background: var(--panel-background);
  backdrop-filter: blur(var(--glass-blur));
}

button {
  font: inherit;
  color: inherit;
  cursor: pointer;
}

button:disabled {
  opacity: 0.4;
  cursor: default;
}

.ghost {
  padding: 6px 12px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.03);
  font-size: 12.5px;
}

.ghost:hover:not(:disabled) {
  background: color-mix(in srgb, var(--accent) 25%, transparent);
}

.ghost.small {
  padding: 2px 9px;
  font-size: 15px;
}

.violet,
.gold {
  padding: 7px 14px;
  border-radius: 999px;
  font-size: 12.5px;
}

.violet {
  border: 1px solid var(--accent);
  background: linear-gradient(135deg, color-mix(in srgb, var(--accent) 70%, #000), var(--accent));
  color: #fff;
  box-shadow: 0 0 14px color-mix(in srgb, var(--accent) 40%, transparent);
}

.gold {
  border: 1px solid #e6bf6a;
  background: linear-gradient(135deg, #e9c46a, #c9973e);
  color: #2a1a05;
}

.hers {
  animation: hers 1.6s ease-out;
}

@keyframes hers {
  0% { box-shadow: 0 0 0 2px rgba(255, 210, 122, 0.9), 0 0 22px rgba(255, 210, 122, 0.6); }
  100% { box-shadow: none; }
}

.song {
  grid-area: song;
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px 12px;
  min-width: 0;
}

.song .glyph {
  font-size: 22px;
  color: var(--accent);
}

.song-names {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
}

.song-names strong,
.song-names small {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.song-names strong {
  font-size: 13.5px;
}

.song-names small {
  font-size: 11px;
  color: var(--text-dim);
}

.rail {
  grid-area: rail;
}

.shelf {
  grid-area: shelf;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
  padding: 10px;
}

.covers {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
  overflow-y: auto;
  min-height: 0;
  padding-right: 2px;
}

.cover {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 3px;
  border: 1px solid transparent;
  border-radius: 10px;
  background: none;
  text-align: left;
}

.cover img,
.cover .blank {
  width: 100%;
  aspect-ratio: 1;
  border-radius: 8px;
  object-fit: cover;
}

.cover .blank {
  display: grid;
  place-items: center;
  background: color-mix(in srgb, var(--accent) 15%, transparent);
  font-size: 22px;
}

.cover small {
  overflow: hidden;
  font-size: 10.5px;
  color: var(--text-dim);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.cover.on {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 22%, transparent);
  box-shadow: 0 0 14px color-mix(in srgb, var(--accent) 40%, transparent);
}

.tracks {
  display: flex;
  flex-direction: column;
  gap: 2px;
  max-height: 30%;
  overflow-y: auto;
  border-top: 1px solid var(--line);
  padding-top: 6px;
}

.tracks button {
  padding: 4px 6px;
  border: 0;
  border-radius: 6px;
  background: none;
  font-size: 11.5px;
  text-align: left;
}

.tracks button.on {
  background: color-mix(in srgb, var(--accent) 30%, transparent);
}

.main {
  grid-area: main;
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-height: 0;
  min-width: 0;
  padding: 14px 16px;
  overflow-y: auto;
}

.transport {
  display: flex;
  align-items: flex-end;
  gap: 12px;
}

.transport .grow {
  flex: 1;
  min-width: 0;
}

.play {
  flex: none;
  width: 46px;
  height: 46px;
  margin-bottom: 36px;
  border: 2px solid color-mix(in srgb, var(--accent) 80%, #fff);
  border-radius: 50%;
  background: color-mix(in srgb, var(--accent) 35%, transparent);
  box-shadow: 0 0 16px color-mix(in srgb, var(--accent) 50%, transparent);
  font-size: 15px;
}

.panel {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.acts {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 10px;
}

.quiet {
  margin: 0;
  font-size: 12.5px;
  font-style: italic;
  color: var(--text-dim);
}

.draft {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px 12px;
  border-radius: 10px;
}

.draft label {
  display: flex;
  flex-direction: column;
  gap: 3px;
  font-size: 10.5px;
  letter-spacing: 0.08em;
  color: var(--text-dim);
}

.draft label.tall {
  grid-column: 1 / -1;
}

.draft input,
.draft textarea,
.loops input,
.form-card input,
.form-card textarea {
  padding: 6px 9px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.06);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

.draft textarea,
.form-card textarea {
  resize: vertical;
  line-height: 1.5;
}

.takes {
  display: flex;
  justify-content: center;
  gap: 14px;
  border-radius: 12px;
}

.take {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  padding: 12px 18px;
  border: 1px solid var(--line);
  border-radius: 12px;
}

.take.on {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 25%, transparent);
}

.take .pick {
  padding: 6px 14px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.05);
}

.take small {
  font-size: 11px;
  color: var(--text-dim);
}

.loops {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  font-size: 12px;
}

.loops strong {
  letter-spacing: 0.12em;
  font-size: 11px;
  color: var(--text-dim);
}

.loops input {
  flex: 1;
  min-width: 160px;
}

.laid {
  font-size: 11.5px;
  color: var(--warn, #ffd59a);
  border-radius: 6px;
}

.x {
  padding: 0 4px;
  border: 0;
  background: none;
}

.finish {
  display: grid;
  grid-template-columns: 1fr auto;
  gap: 14px;
  min-height: 0;
}

.lyrics header {
  display: flex;
  align-items: center;
  gap: 10px;
  margin-bottom: 6px;
  font-size: 11.5px;
  color: var(--text-dim);
}

.karaoke {
  display: flex;
  align-items: center;
  gap: 4px;
  border-radius: 6px;
}

.words {
  max-height: 260px;
  overflow-y: auto;
  padding-right: 6px;
}

.label {
  margin: 10px 0 3px;
  font-size: 10.5px;
  letter-spacing: 0.12em;
  text-transform: uppercase;
  color: var(--text-dim);
}

.line {
  margin: 0;
  padding: 2px 6px;
  border-radius: 6px;
  font-size: 14px;
  line-height: 1.55;
  cursor: pointer;
  transition: background 0.3s, color 0.3s;
}

.line:hover {
  background: rgba(255, 255, 255, 0.05);
}

.line.lit {
  background: color-mix(in srgb, var(--accent) 22%, transparent);
  text-shadow: 0 0 12px color-mix(in srgb, var(--accent) 90%, transparent);
}

.word {
  border-radius: 4px;
  transition: color 0.15s, text-shadow 0.15s, background 0.15s;
}

.word.past {
  color: color-mix(in srgb, var(--accent) 50%, var(--text));
}

.word.lit {
  background: rgba(255, 210, 122, 0.22);
  color: #fff3d6;
  text-shadow: 0 0 10px rgba(255, 210, 122, 0.95);
}

.came-home {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin: 10px 0 0;
  padding: 0;
  list-style: none;
  border-radius: 10px;
}

.came-home li {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 5px 8px;
  border: 1px solid var(--line);
  border-radius: 9px;
  background: rgba(0, 0, 0, 0.25);
  font-size: 12px;
}

.came-home img {
  width: 36px;
  height: 36px;
  border-radius: 6px;
  object-fit: cover;
}

.came-home .kind {
  padding: 1px 6px;
  border-radius: 5px;
  background: color-mix(in srgb, var(--accent) 25%, transparent);
  font-size: 10px;
  letter-spacing: 0.08em;
}

.came-home .file {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.small-text {
  padding: 3px 8px;
  font-size: 11px;
}

.exports {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 170px;
  max-width: 200px;
}

.notice {
  position: sticky;
  bottom: 0;
  margin: 0;
  padding: 7px 12px;
  border-radius: 10px;
  background: var(--panel-background);
  border: 1px solid var(--line);
  font-size: 12.5px;
  text-align: center;
  color: var(--warn, #ffd59a);
}

.chat {
  grid-area: chat;
  position: relative;
  min-height: 0;
  overflow: hidden;
}

.chat .waiting {
  padding: 16px;
}

.dials {
  grid-area: dials;
}

.credits {
  grid-area: credits;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 12px 14px;
}

.coin {
  display: grid;
  place-items: center;
  flex: none;
  width: 44px;
  height: 44px;
  border-radius: 50%;
  background: radial-gradient(circle at 35% 30%, #ffe08a, #c9973e);
  color: #5a3a08;
  font-size: 22px;
  box-shadow: 0 0 16px rgba(255, 200, 100, 0.45);
}

.credit-text {
  display: flex;
  flex: 1;
  flex-direction: column;
  align-items: flex-start;
  gap: 3px;
  min-width: 0;
}

.credit-text small {
  font-size: 11px;
  color: var(--text-dim);
}

.meter {
  align-self: stretch;
  height: 7px;
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.1);
}

.meter span {
  display: block;
  height: 100%;
  border-radius: 4px;
  background: linear-gradient(90deg, color-mix(in srgb, var(--accent) 60%, #000), var(--accent));
}

.bridge {
  position: absolute;
  left: 50%;
  bottom: 2px;
  margin: 0;
  font-size: 11px;
  color: var(--warn, #ffd59a);
  transform: translateX(-50%);
}

.dialog {
  position: absolute;
  inset: 0;
  z-index: 8;
  display: flex;
  align-items: center;
  justify-content: center;
  background: rgba(0, 0, 0, 0.5);
  backdrop-filter: blur(3px);
}

.form-card {
  display: flex;
  flex-direction: column;
  gap: 10px;
  width: min(520px, 90%);
  padding: 20px;
  border: 1px solid var(--line);
  border-radius: 16px;
  background: var(--panel-background);
  backdrop-filter: blur(var(--glass-blur));
}

.form-card h3 {
  margin: 0;
  font-size: 13px;
  letter-spacing: 0.14em;
}

.form-card p {
  margin: 0;
  font-size: 12.5px;
  color: var(--text-dim);
}

.form-card .row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.form-card .row input {
  flex: 1;
  min-width: 0;
}

.form-card .acts {
  justify-content: flex-end;
}
</style>
