<script setup>
// WORLD: a settings room. Each of the member's other screens can show a world - the isles, a
// world made of model pieces, a slideshow - with its own camera, switched on or off here. No
// world is drawn in this room unless the member asks for the preview.
import GlassSelect from '../components/GlassSelect.vue'
import HelpTip from '../components/HelpTip.vue'
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import WorldStage from '../worlds/WorldStage.vue'
import { absPath } from './dev/tree.js'
import { isMac } from '../platform.js'
import { allWorlds, DEFAULT_WORLD, findWorld } from '../worlds/registry.js'
import { statusLine } from '../worlds/readiness.js'
import { checkWorldStates, readWorldBasics, readWorldQuota, worldMine, worldQuota, worldScreensNow, worldSettings, worldStates } from '../composables/useWorld.js'
import { CAMERAS } from '../world3d/director.js'
import { askCompanion } from '../api/ask.js'
import { listen } from '@tauri-apps/api/event'
import { quotaWords } from '../world3d/energy.js'
import { sendLatest } from '../components/sendlatest.js'
import { providerLabel } from '../components/providers.js'
import { castleMoveToScreen, chatMediaOpen, choosePicture, saveSettings, worldApply, worldDelete, worldIdentify, worldNew, worldPrepare, worldScreens } from '../api/castle.js'

const props = defineProps({
  active: { type: Boolean, default: true },
  companion: { type: String, default: 'your companion' },
})
const emit = defineEmits(['show-chat'])

const VIEWS = [
  { id: 'follow', title: 'Follow' },
  { id: 'wide', title: 'Wide' },
  { id: 'close', title: 'Close' },
  { id: 'orbit', title: 'Orbit' },
  { id: 'crane', title: 'Crane' },
]
const DEFAULTS = { world: DEFAULT_WORLD, camera: 'director', on: false, holdMin: 8, holdMax: 20, glide: 'mix', shots: VIEWS.map((v) => v.id), seconds: 15, shuffle: true }
const REFRESH_MS = 10_000

// read at start-up (composables/useWorld.js), so the room opens with its cards and screens there
const settings = worldSettings
const screens = worldScreensNow
const rows = ref([]) // { screen (ScreenInfo), entry (WorldScreen) }
const mine = worldMine
const states = worldStates // world id -> { have, total, ready }
const selected = ref(-1) // the row the cards pick for
const previewId = ref('')
const previewOpen = ref(false)
const menu = ref(-1) // the row whose Settings menu is open
const saved = ref('')
const problem = ref('')
const form = ref({ open: false, name: '', description: '', layout: 'centrepiece', picture: '' })
const told = ref('') // what happened after Create / Ask
const progress = ref({}) // slug -> the prepare helper's last line
const preparing = ref({}) // slug -> true while it runs
const confirmDelete = ref('') // slug waiting for a second press
const quota = worldQuota // undefined = not asked yet, null = unknown
const cliName = computed(() => providerLabel(settings.value?.cli ?? 'claude'))
const quotaLine = computed(() => {
  if (quota.value === undefined) return ''
  if (!quota.value) return `The crystal can't read your ${cliName.value} limit right now, so it keeps its normal glow.`
  return `The crystal shines with your ${cliName.value} weekly limit: ${quotaWords(quota.value)}.`
})
let timer = 0
let savedTimer = 0
let stopProgress = null

const worlds = computed(() => allWorlds(mine.value))
const others = computed(() => rows.value.filter((r) => !r.screen.isMain))
const pickRow = computed(() => rows.value[selected.value] ?? others.value[0] ?? null)
const previewWorld = computed(() => findWorld(worlds.value, previewId.value) ?? findWorld(worlds.value, pickRow.value?.entry.world) ?? findWorld(worlds.value, DEFAULT_WORLD))
watch(
  () => `${selected.value}|${pickRow.value?.entry.world}`,
  () => (previewId.value = ''),
)
const previewConfig = computed(() => ({ ...DEFAULTS, ...(pickRow.value?.entry ?? {}), world: previewWorld.value.id }))

const shortName = (name) => String(name).replace(/^\\\\\.\\/, '')
const ready = (w) => w.engine !== 'glb' || !!states.value[w.id]?.ready
const optionLabel = (w) => (ready(w) ? w.title : `${w.title} (${states.value[w.id]?.have ?? 0}/${states.value[w.id]?.total ?? w.pieces.length})`)
const kindOf = (id) => findWorld(worlds.value, id)?.kind ?? '3d'

// The saved entry for a screen: the same name first, else the same size and place.
function entryFor(screen, entries) {
  return (
    entries.find((e) => e.screen && e.screen === screen.name) ??
    entries.find((e) => e.width === screen.width && e.height === screen.height && e.x === screen.x && e.y === screen.y)
  )
}

function buildRows(keepEdits = true) {
  const entries = settings.value?.world?.screens ?? []
  const before = keepEdits ? rows.value : []
  rows.value = screens.value.map((screen) => {
    const edited = before.find((r) => r.screen.name === screen.name)
    const entry = edited?.entry ?? entryFor(screen, entries) ?? {}
    return { screen, entry: { ...DEFAULTS, ...entry, screen: screen.name, width: screen.width, height: screen.height, x: screen.x, y: screen.y } }
  })
}

async function refresh({ fresh = false } = {}) {
  try {
    await readWorldBasics()
  } catch {
    return // outside Castle's window
  }
  buildRows(!fresh)
  await checkWorldStates(fresh)
  await readWorldQuota()
}

// Every change goes out whole: the rows for the screens here, plus the saved ones for screens
// that are unplugged right now (so they come back as they were).
// Only one send runs at a time. Click fast through the world cards and the screen would otherwise
// reload every world in between, one after another, and trail far behind: while one send is out, the
// newer choices only mark "again", and the latest rows are sent once - the stale ones are skipped.
// A send that fails while newer choices wait does not drop them: the latest is still sent.
const apply = sendLatest(
  async (newer) => {
    problem.value = ''
    const here = rows.value.map((r) => r.entry)
    const away = (settings.value?.world?.screens ?? []).filter((e) => !rows.value.some((r) => r.screen.name === e.screen))
    const list = await worldApply([...here, ...away])
    settings.value = { ...settings.value, world: { ...settings.value.world, screens: list } }
    if (newer()) return // newer choices are waiting: send those, no need to redraw the old ones
    screens.value = await worldScreens()
    buildRows(true)
    window.dispatchEvent(new CustomEvent('castle-music', { detail: { screens: true } })) // hand the music over
    saved.value = 'Saved'
    clearTimeout(savedTimer)
    savedTimer = setTimeout(() => (saved.value = ''), 4000)
  },
  (reason) => (problem.value = String(reason)),
)

function change(row, patch) {
  row.entry = { ...row.entry, ...patch }
  if (row.entry.holdMin > row.entry.holdMax) [row.entry.holdMin, row.entry.holdMax] = [row.entry.holdMax, row.entry.holdMin]
  apply()
}

function setWorld(row, id) {
  const world = findWorld(worlds.value, id)
  if (!world || !ready(world)) return
  // a model world starts on its own camera (Crystal Isle: the slow orbit)
  const camera = world.kind === '3d' && world.camera?.default ? world.camera.default : row.entry.camera
  change(row, { world: id, camera })
}

function toggleView(row, id) {
  const shots = row.entry.shots.includes(id) ? row.entry.shots.filter((s) => s !== id) : [...row.entry.shots, id]
  if (shots.length) change(row, { shots })
}

function reset(row) {
  change(row, { holdMin: DEFAULTS.holdMin, holdMax: DEFAULTS.holdMax, glide: DEFAULTS.glide, shots: [...DEFAULTS.shots], seconds: DEFAULTS.seconds, shuffle: true })
}

async function identify(row) {
  problem.value = ''
  try {
    await worldIdentify(row.screen.index)
  } catch (reason) {
    problem.value = String(reason)
  }
}

// Castle itself moves to this screen (and stays there); a world on it gives way, and the screen
// Castle left may now show one.
async function moveCastle(row) {
  problem.value = ''
  try {
    await castleMoveToScreen(row.screen.index)
    window.dispatchEvent(new CustomEvent('castle-music', { detail: { screens: true } })) // the music hands over too
    await refresh({ fresh: true })
  } catch (reason) {
    problem.value = String(reason)
  }
}

function pickCard(world) {
  previewId.value = world.id
  const row = pickRow.value
  if (row && !row.screen.isMain && ready(world)) setWorld(row, world.id)
}

// ---------- the member's own worlds ----------

const pictureName = computed(() => form.value.picture.split(/[\\/]/).pop())

async function pickPicture() {
  const path = await choosePicture('A picture for the look of your world').catch(() => null)
  if (path) form.value.picture = path
}

// The brief for the companion, sent through the session like typed words.
function paintBrief(world) {
  const f = world.folder
  const at = (rel) => absPath(f, rel)
  const dir = (rel) => at(`${rel}/_`).slice(0, -1) // the folder with its own trailing separator
  // The styles folder as the companion's shell names it: %CASTLE_STYLES% on Windows, $CASTLE_STYLES on a Mac.
  const style = (name) => (isMac ? `$CASTLE_STYLES/${name}` : `%CASTLE_STYLES%\\${name}`)
  const picture = ` If there is a picture.* in ${f}, use it for the look.`
  return [
    `Please help me build my 3D world "${world.title}" for Castle's WORLD room.`,
    `Read ${at('request.md')} (my words) and ${at('world.json')} (Castle's draft of the pieces).`,
    '1. Polish world.json: change only "title", "line" (one sentence) and "glow" (a colour like #8fe6ff). Keep every piece exactly as it is.',
    `2. Paint the concept with the style in ${style('world-concept.md')} as ${at('design/00-concept.png')}, then one picture per piece with ${style('world-piece.md')}, each named like its piece file with .png instead of .glb (for example 01-base-isle.png), in ${dir('design')}.${picture}`,
    `3. Tell me the cost first and ask before you draw. If pictures cannot be made, write one prompt per piece into ${at('design/prompts.md')} instead.`,
    `When you are done, tell me to make each picture in Tripo, export it as a GLB named with the same number, put it in ${dir('hd')} and press Prepare pieces in the WORLD room.`,
  ]
}

async function askToPaint(world) {
  try {
    await askCompanion(paintBrief(world), props.companion)
    told.value = `Asked ${props.companion} to paint "${world.title}". Follow along in Chat.`
    emit('show-chat')
  } catch (reason) {
    told.value = String(reason?.message ?? reason)
  }
}

async function create() {
  problem.value = ''
  told.value = ''
  const { name, description, layout, picture } = form.value
  try {
    const made = await worldNew(name, description, layout, picture || null)
    form.value = { open: false, name: '', description: '', layout: 'centrepiece', picture: '' }
    await refresh()
    const entry = mine.value.find((w) => w.slug === made.slug)
    if (entry) await askToPaint(entry)
  } catch (reason) {
    problem.value = String(reason?.message ?? reason)
  }
}

function myStatus(w) {
  if (w.have >= w.pieces.length && w.pieces.length) return 'ready'
  if (preparing.value[w.slug]) return progress.value[w.slug] || 'preparing…'
  if (w.exports >= w.pieces.length) return 'every piece is in - press Prepare pieces'
  if (w.painted) return `pictures painted · ${w.exports}/${w.pieces.length} made in Tripo`
  return `waiting for its pictures · ${w.exports}/${w.pieces.length} made in Tripo`
}

async function prepare(w) {
  problem.value = ''
  preparing.value = { ...preparing.value, [w.slug]: true }
  progress.value = { ...progress.value, [w.slug]: 'starting…' }
  try {
    await worldPrepare(w.slug)
  } catch (reason) {
    preparing.value = { ...preparing.value, [w.slug]: false }
    problem.value = String(reason?.message ?? reason)
  }
}

async function remove(w) {
  if (confirmDelete.value !== w.slug) {
    confirmDelete.value = w.slug
    return
  }
  confirmDelete.value = ''
  try {
    await worldDelete(w.slug)
    // a screen showing it goes back to the built-in isles
    for (const row of rows.value.filter((r) => r.entry.world === w.id)) row.entry = { ...row.entry, world: DEFAULT_WORLD }
    await apply()
    await refresh({ fresh: true })
  } catch (reason) {
    problem.value = String(reason?.message ?? reason)
  }
}

const openFolder = (w, sub) => chatMediaOpen(sub ? absPath(w.folder, sub) : w.folder).catch((reason) => (problem.value = String(reason)))

onMounted(async () => {
  refresh()
  timer = setInterval(() => props.active && refresh(), REFRESH_MS)
  try {
    stopProgress = await listen('world-prepare', async ({ payload }) => {
      progress.value = { ...progress.value, [payload.slug]: payload.line }
      if (payload.done) {
        preparing.value = { ...preparing.value, [payload.slug]: false }
        if (!payload.ok) problem.value = payload.line || 'The pieces could not be prepared.'
        await refresh({ fresh: true })
      }
    })
  } catch {
    // outside Castle's window
  }
})
onBeforeUnmount(() => {
  clearInterval(timer)
  clearTimeout(savedTimer)
  stopProgress?.()
})
watch(
  () => props.active,
  (on) => {
    if (on) refresh()
    else previewOpen.value = false
  },
)
</script>

<template>
  <div class="world-room" @click="menu = -1">
    <div class="panels">
      <section class="widget screens">
        <div class="bar">
          <h2 class="with-tip">YOUR SCREENS<HelpTip v-if="settings" text="The music switch is at the top right, next to Settings. A screen showing a world plays that world's own music; Castle's music steps aside while one is open." /></h2>
          <span class="spacer"></span>
          <span v-if="saved" class="good">{{ saved }}</span>
          <button @click="refresh({ fresh: true })">Look for screens</button>
        </div>

        <div v-for="(row, i) in rows" :key="row.screen.name" class="screen-row" :class="{ picked: pickRow === row && !row.screen.isMain }" @click="!row.screen.isMain && (selected = i)">
          <span class="number">{{ row.screen.index + 1 }}</span>
          <div class="who">
            <strong>{{ shortName(row.screen.name) }}</strong>
            <span class="hint">{{ row.screen.width }}×{{ row.screen.height }}{{ row.screen.primary ? ' · primary' : '' }}</span>
            <label class="castle-pick" :class="{ lives: row.screen.isMain }" :title="row.screen.isMain ? 'Castle itself lives on this screen' : 'Put Castle itself on this screen'" @click.stop>
              <input type="radio" name="castle-screen" :checked="row.screen.isMain" @change="!row.screen.isMain && moveCastle(row)" />
              <span>{{ row.screen.isMain ? 'Castle lives here' : 'Castle' }}</span>
            </label>
          </div>
          <template v-if="!row.screen.isMain">
            <label class="field">
              <span>WORLD</span>
              <GlassSelect :model-value="row.entry.world" :options="worlds.map((w) => ({ value: w.id, label: optionLabel(w), disabled: !ready(w) }))" aria-label="World" @update:model-value="(v) => setWorld(row, v)" />
            </label>
            <label class="field">
              <span>CAMERA</span>
              <GlassSelect v-if="kindOf(row.entry.world) === '3d'" :model-value="row.entry.camera" :options="CAMERAS.map((c) => ({ value: c.id, label: c.title }))" aria-label="Camera" @update:model-value="(v) => change(row, { camera: v })" />
              <GlassSelect v-else model-value="slideshow" :options="[{ value: 'slideshow', label: 'Slideshow' }]" disabled aria-label="Camera" />
            </label>
            <button @click.stop="identify(row)">Identify</button>
            <div class="menu-anchor">
              <button @click.stop="menu = menu === i ? -1 : i">Settings ▾</button>
              <div v-if="menu === i" class="menu widget" @click.stop>
                <template v-if="kindOf(row.entry.world) === '3d'">
                  <p class="menu-title">Each view holds</p>
                  <div class="row">
                    <input type="number" min="3" max="120" :value="row.entry.holdMin" @change="change(row, { holdMin: Math.max(3, Math.min(120, +$event.target.value || 8)) })" />
                    <span>to</span>
                    <input type="number" min="3" max="120" :value="row.entry.holdMax" @change="change(row, { holdMax: Math.max(3, Math.min(120, +$event.target.value || 20)) })" />
                    <span>seconds</span>
                  </div>
                  <p class="menu-title">Between views</p>
                  <div class="row">
                    <label v-for="g in ['glide', 'cut', 'mix']" :key="g" class="choice"><input type="radio" :checked="row.entry.glide === g" @change="change(row, { glide: g })" /> {{ g }}</label>
                  </div>
                  <p class="menu-title">Views the director uses</p>
                  <div class="row">
                    <label v-for="v in VIEWS" :key="v.id" class="choice"><input type="checkbox" :checked="row.entry.shots.includes(v.id)" @change="toggleView(row, v.id)" /> {{ v.title }}</label>
                  </div>
                </template>
                <template v-else>
                  <p class="menu-title">Each picture stays</p>
                  <div class="row">
                    <input type="number" min="4" max="120" :value="row.entry.seconds" @change="change(row, { seconds: Math.max(4, Math.min(120, +$event.target.value || 15)) })" />
                    <span>seconds</span>
                  </div>
                  <label class="choice"><input type="checkbox" :checked="row.entry.shuffle" @change="change(row, { shuffle: $event.target.checked })" /> Shuffle</label>
                </template>
                <div class="row end">
                  <button @click="reset(row)">Reset to default</button>
                  <button @click="menu = -1">Close</button>
                </div>
              </div>
            </div>
            <button
              class="flip push-right"
              :class="{ on: row.entry.on }"
              role="switch"
              :aria-checked="row.entry.on"
              :aria-label="row.entry.on ? 'Switch this screen off' : 'Show the world on this screen'"
              :title="row.entry.on ? 'On: click to switch this screen off' : 'Off: click to show the world on this screen'"
              @click.stop="change(row, { on: !row.entry.on })"
            >
              <span class="knob"></span>
            </button>
          </template>
        </div>

        <p v-if="!others.length && rows.length" class="hint">Only one screen is connected. Plug in another screen to give it a world, or preview a world here.</p>
        <p v-else-if="others.length" class="hint">Click a screen, then a world on the right to show it there.</p>
        <p v-if="problem" class="problem">{{ problem }}</p>
        <p v-if="quotaLine" class="hint">{{ quotaLine }}</p>
      </section>

      <section class="widget worlds">
        <div class="bar has-tip">
          <h2>WORLDS</h2>
          <span class="spacer"></span>
          <button @click.stop="form.open = !form.open">{{ form.open ? 'Close' : '+ Create world' }}</button>
          <HelpTip block :text="`Describe a world and ${companion} paints a picture of each piece. You make each picture into a 3D model in Tripo, and Castle prepares them for your screens.`" />
        </div>

        <div v-if="form.open" class="create" @click.stop>
          <label class="wide">Name <input v-model="form.name" maxlength="60" placeholder="Star Garden" /></label>
          <label class="wide">What is it like? <textarea v-model="form.description" rows="3" maxlength="600" placeholder="A moonlit garden of glowing flowers on floating rocks, with a silver tree at its heart"></textarea></label>
          <div class="row">
            <label class="choice"><input v-model="form.layout" type="radio" value="centrepiece" /> One centrepiece - a great isle, the heart of it glowing, three islets circling</label>
          </div>
          <div class="row">
            <label class="choice"><input v-model="form.layout" type="radio" value="archipelago" /> An archipelago - five isles joined by bridges of light, the crystal walking between them</label>
          </div>
          <div class="row">
            <button @click="pickPicture">{{ form.picture ? 'Change picture' : 'Add a picture for the look' }}</button>
            <span class="hint">{{ form.picture ? pictureName : 'optional' }}</span>
            <span class="spacer"></span>
            <button class="primary" :disabled="!form.name.trim()" @click="create">Create and ask {{ companion }}</button>
          </div>
        </div>
        <p v-if="told" class="good">{{ told }}</p>

        <div class="grid">
          <article
            v-for="w in worlds"
            :key="w.id"
            class="card"
            :class="{ waiting: !ready(w), chosen: previewWorld.id === w.id }"
            @click="pickCard(w)"
          >
            <div class="preview" :style="w.preview ? { backgroundImage: `url(&quot;${w.preview}&quot;)` } : {}">
              <span class="kind">{{ w.kind === '2d' ? '2D' : '3D' }}</span>
              <span v-if="pickRow?.entry.world === w.id && previewWorld.id !== w.id" class="here">on screen {{ pickRow.screen.index + 1 }}</span>
            </div>
            <strong>{{ w.title }}</strong>
            <p class="desc">{{ w.line }}</p>
            <p class="status">{{ w.source === 'yours' && w.engine === 'glb' ? myStatus(w) : statusLine(w, states[w.id]) }}</p>
            <div v-if="w.source === 'yours' && w.engine === 'glb'" class="row mine" @click.stop>
              <button @click="askToPaint(w)">{{ w.painted ? 'Paint again' : 'Ask to paint' }}</button>
              <button @click="openFolder(w, 'design')">Pictures</button>
              <button @click="openFolder(w, 'hd')">Tripo folder</button>
              <button :disabled="w.exports < w.pieces.length || preparing[w.slug]" :title="w.exports < w.pieces.length ? 'Put every piece from Tripo in the Tripo folder first' : ''" @click="prepare(w)">Prepare pieces</button>
              <button :class="{ danger: confirmDelete === w.slug }" @click="remove(w)">{{ confirmDelete === w.slug ? 'Press again to delete' : 'Delete' }}</button>
            </div>
          </article>
        </div>
        <div class="row">
          <button @click="previewOpen = !previewOpen">{{ previewOpen ? 'Close the preview' : 'Preview the picked world here' }}</button>
        </div>
      </section>
    </div>


    <section v-if="previewOpen" class="widget preview-panel">
      <WorldStage :config="previewConfig" :mine="mine" :active="active" />
      <span class="preview-name">{{ previewWorld.title }}</span>
    </section>
  </div>
</template>

<style scoped>
.world-room {
  position: absolute;
  inset: 24px;
  display: flex;
  flex-direction: column;
  gap: 16px;
  overflow-y: auto;
}
.panels {
  display: grid;
  grid-template-columns: minmax(520px, 1fr) minmax(420px, 1fr);
  gap: 16px;
  align-items: start;
}
.widget {
  background: var(--panel-background);
  border: 1px solid var(--line);
  border-radius: 14px;
  -webkit-backdrop-filter: blur(var(--glass-blur)); backdrop-filter: blur(var(--glass-blur));
}
.screens,
.worlds {
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 16px 18px;
}
.bar,
.row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
}
.row.end {
  justify-content: flex-end;
  margin-top: 6px;
}
.spacer {
  flex: 1;
}
/* a row with a (?) whose bubble spans the row (HelpTip block) */
.has-tip {
  position: relative;
}
h2.with-tip {
  display: flex;
  align-items: center;
}
h2 {
  margin: 0;
  font-size: 13px;
  font-weight: 500;
  letter-spacing: 0.2em;
  color: var(--accent);
}
.screen-row {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: flex-end;
  padding: 12px;
  border: 1px solid var(--line);
  border-radius: 10px;
  cursor: pointer;
}
.screen-row.picked {
  border-color: var(--accent);
}
.number {
  display: grid;
  place-items: center;
  width: 30px;
  height: 30px;
  border-radius: 8px;
  background: var(--accent);
  color: #07142e;
  font-weight: 700;
  align-self: center;
}
.who {
  display: flex;
  flex-direction: column;
  min-width: 130px;
  align-self: center;
}
.lives {
  align-self: center;
  font-style: italic;
  color: var(--text-dim);
  font-size: 13px;
}
.field {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 11px;
  color: var(--text-dim);
  letter-spacing: 0.08em;
}
input[type='number'] {
  padding: 6px 8px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.3);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}
input[type='number'] {
  width: 64px;
}
.menu-anchor {
  position: relative;
}
.menu {
  position: absolute;
  right: 0;
  top: calc(100% + 6px);
  z-index: 5;
  width: 300px;
  padding: 12px 14px;
  cursor: default;
}
.menu-title {
  margin: 8px 0 4px;
  font-size: 12px;
  color: var(--text-dim);
}
.choice {
  display: flex;
  gap: 4px;
  align-items: center;
  font-size: 13px;
}
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 12px;
}
.card {
  display: flex;
  flex-direction: column;
  gap: 4px;
  padding: 8px;
  border: 1px solid var(--line);
  border-radius: 10px;
  cursor: pointer;
}
.card.chosen {
  border-color: var(--accent);
}
.card.waiting .preview {
  filter: grayscale(0.85) brightness(0.7);
}
.preview {
  position: relative;
  height: 110px;
  border-radius: 8px;
  background: linear-gradient(135deg, #1b2050, #3a2a6a);
  background-size: cover;
  background-position: center;
}
.kind {
  position: absolute;
  top: 6px;
  left: 6px;
  padding: 1px 7px;
  border-radius: 999px;
  background: rgba(4, 8, 23, 0.7);
  font-size: 11px;
}
.here {
  position: absolute;
  top: 6px;
  right: 6px;
  padding: 1px 7px;
  border-radius: 999px;
  background: rgba(4, 8, 23, 0.7);
  font-size: 11px;
  color: var(--text-dim);
}
.desc,
.status,
.hint {
  margin: 0;
  font-size: 12px;
  color: var(--text-dim);
}
.status {
  color: var(--accent);
}
.good {
  font-size: 12px;
  color: var(--good);
}
.problem {
  margin: 0;
  font-size: 12px;
  color: var(--warn);
}
.create {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 12px;
  border: 1px dashed var(--line);
  border-radius: 10px;
}
.create label.wide {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 13px;
}
.create input,
.create textarea {
  padding: 7px 9px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.3);
  color: var(--text);
  font: inherit;
  font-size: 13px;
  resize: vertical;
}
.row.mine button {
  padding: 3px 8px;
  font-size: 11px;
}
button:disabled {
  opacity: 0.45;
  cursor: default;
}
button.danger {
  border-color: var(--warn);
  color: var(--warn);
}
.preview-panel {
  position: relative;
  flex: 0 0 auto;
  height: 460px;
  overflow: hidden;
}
.preview-name {
  position: absolute;
  top: 12px;
  left: 14px;
  padding: 3px 10px;
  border-radius: 8px;
  background: rgba(4, 8, 23, 0.6);
  font-size: 12px;
}
button {
  padding: 6px 14px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}
button.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: #07142e;
  font-weight: 600;
}
.castle-pick {
  display: inline-flex;
  align-self: flex-start;
  align-items: center;
  margin-top: 4px;
  gap: 6px;
  flex: none;
  white-space: nowrap;
  font-size: 13px;
  color: var(--text-dim);
  cursor: pointer;
}
.castle-pick input {
  margin: 0;
  accent-color: var(--accent);
  cursor: pointer;
}
.castle-pick:has(input:checked) {
  color: var(--text);
}
.flip.push-right {
  margin-left: auto;
  align-self: center;
}
.flip {
  position: relative;
  flex: none;
  width: 42px;
  height: 24px;
  min-width: 42px;
  padding: 0;
  border-radius: 12px;
  border: 1px solid var(--line, rgba(255, 255, 255, 0.2));
  background: rgba(255, 255, 255, 0.12);
  cursor: pointer;
  transition: background 0.2s ease;
}
.flip .knob {
  position: absolute;
  top: 3px;
  left: 3px;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: #fff;
  transition: transform 0.2s ease;
}
.flip.on {
  background: var(--accent, #9b7dff);
}
.flip.on .knob {
  transform: translateX(18px);
}
@media (max-width: 1100px) {
  .panels {
    grid-template-columns: 1fr;
  }
}
</style>
