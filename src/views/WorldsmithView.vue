<script setup>
// WORLDSMITH (S-1126-7 / S-1126-9, his pick worldsmith-9): the AI cheat engine for the member's own
// offline single-player games. The newest screenshot fills the canvas with every value the companion
// found pinned on it (a Lock on each); the room's own session is the chat on the right; the games and
// the recipes sit in the dock below. The small memory helper is fetched only when the member presses
// Get it, starts only while the room is open, and lets the game go when the room closes.
// The room is the Forge: each feature is a Tool (forge.js) - a Hammer changes a running game, a Lens
// only looks at the screen and draws on top. A chooser opens each time the room does; Values is the
// first Hammer (the canvas below), Translate the first Lens (the LINES column and the overlay window).
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import TerminalPanel from '../components/TerminalPanel.vue'
import {
  onWorldsmithInstall,
  onWorldsmithLines,
  onWorldsmithShot,
  sendMessage,
  worldsmithCall,
  worldsmithClose,
  worldsmithGame,
  worldsmithGetHelper,
  worldsmithMemory,
  worldsmithPrepare,
  worldsmithRecipe,
  worldsmithSnap,
  worldsmithStart,
  worldsmithStatus,
  worldsmithStop,
  worldsmithAutoSet,
  worldsmithModeSet,
  worldsmithOverlayHide,
  worldsmithTranslate,
} from '../api/castle.js'
import { helperWord, lockCall, readGames, readState, recipeWord, shotWord, stepsLine, tagPlaces } from './worldsmith/worldsmith.js'
import { TOOLS, available, lastTool, readFeatures, rememberTool, toolById, toolLabel } from './worldsmith/forge.js'

const props = defineProps({
  companion: { type: String, default: 'Your companion' },
  cli: { type: String, default: 'claude' },
  visible: { type: Boolean, default: true },
})

const ready = ref(false)
const stamp = ref(0)
const problem = ref('')
const note = ref('')
const helper = ref(null)
const getting = ref(false)
const progress = ref(null)
const games = ref([])
const attached = ref(null)
const gameId = ref(null)
const memory = ref({ games: [], values: [], recipes: [] })
const state = ref({ attached: null, labels: [] })
const shot = ref('')
const detailsOpen = ref(false)
// the Forge: which Tool is open, the chooser, what the helper can do, and the Translate Lens's state
const storage = (() => {
  try {
    return window.localStorage
  } catch {
    return null
  }
})()
const tool = ref(lastTool(storage))
const lastId = ref(tool.value)
const choosing = ref(true)
const features = ref([])
const translation = ref(null)
const translating = ref(false)
const auto = ref(false)
let pollTimer = 0
const stops = []

const tags = computed(() => tagPlaces(state.value.labels, memory.value.values))
const keptRecipes = computed(() => memory.value.recipes.filter((r) => r.state === 'kept'))
const proposed = computed(() => memory.value.recipes.filter((r) => r.state === 'proposed'))
const helperLine = computed(() => helperWord(helper.value))
const shotSrc = computed(() => (shot.value ? convertFileSrc(shot.value) : ''))

const say = (reason) => (problem.value = String(reason ?? ''))

const toolCards = computed(() => TOOLS.map((t) => ({ ...t, ...available(t, features.value), label: toolLabel(t) })))
const current = computed(() => toolById(tool.value) ?? TOOLS[0])
const lines = computed(() => (translation.value?.lines ?? []).filter((l) => String(l.text ?? '').trim() || String(l.translated ?? '').trim()))
// a full-screen game: the picture came from the whole screen and nothing could be read over it
const screenOnly = computed(() => translation.value?.via === 'screen' && !lines.value.length)

async function pick(id) {
  const next = toolById(id)
  if (!next || !available(next, features.value).ok) return
  const leaving = tool.value === 'translate' && id !== 'translate'
  tool.value = id
  lastId.value = id
  choosing.value = false
  rememberTool(storage, id)
  if (leaving) {
    stopAuto()
    worldsmithOverlayHide().catch(() => {})
  }
  try {
    await worldsmithModeSet(id)
  } catch (reason) {
    say(reason)
  }
}

/** The chooser closed without a pick: the open tool stays, or Values when it can't run. */
function keepTool() {
  const open = toolById(tool.value)
  pick(open && available(open, features.value).ok ? open.id : 'values')
}

async function readCaps() {
  try {
    features.value = readFeatures(await worldsmithCall('caps'))
  } catch {
    // an older helper has no caps verb: it offers only the Values tool
    features.value = []
  }
}

async function translate() {
  translating.value = true
  say('')
  try {
    const answer = await worldsmithTranslate()
    if (answer) translation.value = answer
  } catch (reason) {
    say(reason)
  } finally {
    translating.value = false
  }
}

async function hideOverlay() {
  try {
    await worldsmithOverlayHide()
  } catch (reason) {
    say(reason)
  }
}

async function setAuto(on) {
  try {
    await worldsmithAutoSet(on)
    auto.value = Boolean(on)
  } catch (reason) {
    auto.value = false
    say(reason)
  }
}

/** Auto off in the page; the core stops it by itself too when the tool or the game goes. */
function stopAuto() {
  if (!auto.value) return
  auto.value = false
  worldsmithAutoSet(false).catch(() => {})
}

function heardLines(payload) {
  if (!payload) return
  if (payload.error) return say(payload.error)
  if (payload.hidden) return
  if (Array.isArray(payload.lines)) {
    translation.value = payload
    say('')
  }
}

async function prepare() {
  try {
    await worldsmithPrepare()
    ready.value = true
    stamp.value += 1
  } catch (reason) {
    say(reason)
  }
  await refreshHelper()
}

async function refreshHelper() {
  try {
    helper.value = await worldsmithStatus()
    if (helper.value.installed && !helper.value.running) await worldsmithStart()
    if (helper.value.installed) await readCaps()
    if (helper.value.installed) await loadGames()
  } catch (reason) {
    say(reason)
  }
}

async function getHelper() {
  getting.value = true
  say('')
  try {
    helper.value = await worldsmithGetHelper()
    await refreshHelper()
  } catch (reason) {
    say(reason)
  } finally {
    getting.value = false
    progress.value = null
  }
}

async function loadGames() {
  try {
    memory.value = await worldsmithMemory(gameId.value)
    games.value = readGames(await worldsmithCall('games'), memory.value.games)
  } catch (reason) {
    say(reason)
  }
}

async function attach(game) {
  if (game.refused) return say(game.reason)
  say('')
  note.value = ''
  try {
    const answer = await worldsmithCall('attach', [game.pid])
    attached.value = game
    if (answer?.note) note.value = answer.note
    gameId.value = await worldsmithGame(game.exe, game.title, game.stamp)
    memory.value = await worldsmithMemory(gameId.value)
    startPolling()
  } catch (reason) {
    say(reason)
  }
}

async function detach() {
  stopPolling()
  stopAuto()
  try {
    await worldsmithCall('detach')
  } catch {
    // the helper may already have let go (the game ended)
  }
  attached.value = null
  state.value = { attached: null, labels: [] }
}

function startPolling() {
  stopPolling()
  pollTimer = setInterval(pollState, 1000)
  pollState()
}

function stopPolling() {
  clearInterval(pollTimer)
  pollTimer = 0
}

async function pollState() {
  try {
    const next = readState(await worldsmithCall('state'))
    state.value = next
    // the game ended by itself: the helper let go
    if (!next.attached && attached.value) {
      attached.value = null
      stopPolling()
      note.value = 'The game closed, so WORLDSMITH let it go.'
    }
  } catch {
    // one missed poll is fine; the next one tries again
  }
}

async function refreshMemory() {
  if (gameId.value) memory.value = await worldsmithMemory(gameId.value).catch(() => memory.value)
}

async function useShot(path) {
  shot.value = path
  try {
    await sendMessage(shotWord(path), 'worldsmith')
  } catch (reason) {
    say(reason)
  }
}

async function snap() {
  try {
    await useShot(await worldsmithSnap())
  } catch (reason) {
    say(reason)
  }
}

async function lock(tag) {
  const call = lockCall(tag)
  if (!call) return
  try {
    await worldsmithCall(call[0], call[1])
    pollState()
  } catch (reason) {
    say(reason)
  }
}

async function useRecipe(recipe) {
  try {
    await sendMessage(recipeWord(recipe), 'worldsmith')
  } catch (reason) {
    say(reason)
  }
}

async function decide(recipe, keep) {
  try {
    await worldsmithRecipe(recipe.id, keep)
    await refreshMemory()
  } catch (reason) {
    say(reason)
  }
}

onMounted(async () => {
  stops.push(await onWorldsmithShot((payload) => (payload?.path ? useShot(payload.path) : say(payload?.error))))
  stops.push(await onWorldsmithInstall((p) => (progress.value = p)))
  stops.push(await onWorldsmithLines(heardLines))
  await prepare()
})

onBeforeUnmount(() => {
  stopPolling()
  stopAuto()
  stops.forEach((stop) => stop())
  // the room closed: the game is let go, every frozen value stops, the session ends
  worldsmithStop().catch(() => {})
  worldsmithClose().catch(() => {})
})

// the companion remembers values and pins as it works: the canvas and the recipes follow
watch(() => state.value.labels.map((l) => `${l.label}:${l.steps.length}`).join(','), refreshMemory)
watch(() => props.visible, (on) => {
  if (!on) return
  // each time the room opens, the chooser asks which tool
  choosing.value = true
  if (helper.value?.installed) loadGames()
})
// the game closed by itself: Auto has nothing left to read
watch(attached, (game) => game || stopAuto())
</script>

<template>
  <div class="worldsmith">
    <header class="title">
      <span class="mark" aria-hidden="true">✦</span>
      <h2>WORLDSMITH</h2>
      <span class="current-tool">{{ current.name }}</span>
      <button class="tools-button" title="Tools" aria-label="Tools" :aria-expanded="choosing" @click="choosing ? keepTool() : (choosing = true)">
        <svg viewBox="0 0 24 24" aria-hidden="true" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round">
          <rect x="3.5" y="3.5" width="7" height="7" rx="1.6" />
          <rect x="13.5" y="3.5" width="7" height="7" rx="1.6" />
          <rect x="3.5" y="13.5" width="7" height="7" rx="1.6" />
          <path d="M17 13.2l1.1 2.7 2.7 1.1-2.7 1.1-1.1 2.7-1.1-2.7-2.7-1.1 2.7-1.1z" fill="currentColor" stroke="none" />
        </svg>
      </button>
    </header>

    <p v-if="problem" class="problem" role="alert">{{ problem }}</p>
    <p v-if="note" class="note">{{ note }}</p>

    <div class="stage">
      <section class="canvas" :aria-label="tool === 'translate' ? 'The lines read from the game' : 'The newest screenshot with the values found'">
        <div v-if="choosing" class="chooser" role="dialog" aria-label="Choose a tool">
          <div class="chooser-head">
            <h3>Choose a tool</h3>
            <button class="link" @click="keepTool">Keep {{ current.name }}</button>
          </div>
          <div class="cards">
            <button
              v-for="t in toolCards"
              :key="t.id"
              class="tool-card"
              :class="{ last: t.id === lastId, dim: !t.ok }"
              :disabled="!t.ok"
              :title="t.ok ? t.label : `Needs a newer helper (${t.missing.join(', ')})`"
              @click="pick(t.id)"
            >
              <svg v-if="t.icon === 'anvil'" class="tool-icon" viewBox="0 0 48 48" aria-hidden="true">
                <path d="M4 20H38V24Q31 25 30 28V32H35V37H13V32H18V28Q17 25 10 24Z" fill="currentColor" />
                <path d="M39 3L40.6 8.4 46 10 40.6 11.6 39 17 37.4 11.6 32 10 37.4 8.4Z" class="spark" />
              </svg>
              <svg v-else class="tool-icon" viewBox="0 0 48 48" aria-hidden="true">
                <path d="M8 7H40A4 4 0 0 1 44 11V30A4 4 0 0 1 40 34H22L13 41V34H8A4 4 0 0 1 4 30V11A4 4 0 0 1 8 7Z" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linejoin="round" />
                <text x="15" y="26" font-size="13" text-anchor="middle" fill="currentColor">&#x3042;</text>
                <text x="32" y="26" font-size="13" font-weight="600" text-anchor="middle" class="spark-text">A</text>
              </svg>
              <strong>{{ t.name }}</strong>
              <span class="tool-line">{{ t.line }}</span>
              <small class="kind">{{ t.label }}</small>
              <small v-if="!t.ok" class="needs">Needs a newer helper</small>
            </button>
          </div>
        </div>
        <div v-if="helper && !helper.installed" class="first-use">
          <h3>A small helper first</h3>
          <p>{{ helperLine }}</p>
          <button v-if="helper.windows && helper.released" class="primary" :disabled="getting" @click="getHelper">{{ getting ? 'Getting it...' : 'Get it' }}</button>
          <progress v-if="progress" :value="progress.done" :max="progress.total || 1"></progress>
        </div>
        <template v-else-if="tool === 'values'">
          <div class="picture">
            <img v-if="shotSrc" :src="shotSrc" alt="The game's newest screenshot" />
            <div v-else class="empty">
              <p v-if="attached">Press <kbd>F9</kbd> in the game, or the Screenshot button, and {{ companion }} reads its numbers.</p>
              <p v-else>Pick your game below to begin. Only your own offline single-player games.</p>
            </div>
            <div
              v-for="tag in tags"
              :key="tag.label"
              class="tag"
              :class="{ found: tag.found, locked: tag.locked }"
              :style="{ left: `${tag.x * 100}%`, top: `${tag.y * 100}%` }"
            >
              <strong>{{ tag.label }} {{ tag.value }}</strong>
              <small>{{ stepsLine(tag.steps) }}</small>
              <button v-if="tag.found" :aria-pressed="tag.locked" @click="lock(tag)">{{ tag.locked ? 'Unlock' : 'Lock' }}</button>
            </div>
          </div>
          <div class="canvas-bar">
            <button :disabled="!attached" @click="snap">Screenshot</button>
            <button :disabled="!state.labels.length" @click="detailsOpen = !detailsOpen">{{ detailsOpen ? 'Hide details' : 'Details' }}</button>
            <span class="spacer"></span>
            <button v-if="attached" @click="detach">Let {{ attached.title }} go</button>
          </div>
          <table v-if="detailsOpen" class="details">
            <thead>
              <tr><th>VALUE</th><th>ADDRESS</th><th>NOW</th><th>SEARCH</th><th>LOCK</th></tr>
            </thead>
            <tbody>
              <tr v-for="tag in state.labels" :key="tag.label">
                <td>{{ tag.label }}</td>
                <td><code>{{ tag.address || '-' }}</code></td>
                <td>{{ tag.value }}</td>
                <td>{{ stepsLine(tag.steps) }}</td>
                <td><button :disabled="!tag.found" @click="lock(tag)">{{ tag.locked ? 'Unlock' : 'Lock' }}</button></td>
              </tr>
            </tbody>
          </table>
        </template>
        <template v-else>
          <div class="lines-head">
            <h3>LINES</h3>
            <small>{{ toolLabel(current) }}</small>
          </div>
          <ol v-if="lines.length" class="lines">
            <li v-for="(line, i) in lines" :key="i">
              <small class="original">{{ line.text }}</small>
              <span class="translated">{{ line.translated }}</span>
            </li>
          </ol>
          <div v-else class="empty lines-empty">
            <p v-if="screenOnly">Set the game to borderless or windowed - a full-screen game can't be drawn over.</p>
            <p v-else-if="attached">Press <kbd>F10</kbd> in the game, or the Translate button, and its words appear here and over the game.</p>
            <p v-else>Pick your game below to begin. Translate only looks at the screen - the game is never touched.</p>
          </div>
          <div class="canvas-bar">
            <button class="primary" :disabled="!attached || translating" @click="translate">{{ translating ? 'Reading...' : 'Translate' }}</button>
            <button @click="hideOverlay">Hide</button>
            <label class="auto">
              <input type="checkbox" role="switch" :checked="auto" :disabled="!attached" @change="setAuto($event.target.checked)" />
              Auto
            </label>
            <span class="spacer"></span>
            <button v-if="attached" @click="detach">Let {{ attached.title }} go</button>
          </div>
          <p class="key-hint">Press <kbd>F10</kbd> in the game to translate, again to hide</p>
        </template>
      </section>

      <section class="chat" aria-label="WORLDSMITH's chat" style="--panel-left: 0px; --panel-top: 0px; --panel-bottom: 0px; --panel-width: 100%">
        <TerminalPanel v-if="ready" room="worldsmith" :settings-stamp="stamp" :companion="companion" :cli="cli" @setup="prepare" @settings="prepare" />
      </section>
    </div>

    <footer class="dock">
      <div class="games" role="list" aria-label="Your games">
        <button
          v-for="game in games"
          :key="game.pid"
          role="listitem"
          class="game"
          :class="{ on: attached?.pid === game.pid, refused: game.refused }"
          :title="game.refused ? game.reason : game.title"
          @click="attach(game)"
        >
          <span class="initial">{{ game.title.slice(0, 1) }}</span>
          <span class="name">{{ game.title }}</span>
          <small v-if="game.remembered" class="remembered">remembered</small>
        </button>
        <p v-if="helper?.installed && !games.length" class="hint">No game window is open. Start your game, then <button class="link" @click="loadGames">look again</button>.</p>
      </div>
      <div v-if="tool === 'values'" class="recipes" aria-label="Recipes">
        <button v-for="r in keptRecipes" :key="r.id" class="recipe" :disabled="!attached" @click="useRecipe(r)">{{ r.name }}</button>
        <div v-for="r in proposed" :key="r.id" class="recipe proposed">
          <span>{{ r.name }}?</span>
          <button @click="decide(r, true)">Keep</button>
          <button @click="decide(r, false)">No</button>
        </div>
      </div>
    </footer>
  </div>
</template>

<style scoped>
.worldsmith {
  /* fills the screen the way every room does (a percent height has nothing to measure against) */
  position: absolute;
  inset: 0;
  /* a column, so the stage takes what is left whether the problem and note lines show or not
     (a five-row grid put the stage in an auto row whenever they were hidden) */
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 14px 18px;
  box-sizing: border-box;
}
.title {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 12px;
}
.title {
  position: relative;
}
.current-tool {
  font-size: 0.8rem;
  letter-spacing: 0.2em;
  text-transform: uppercase;
  opacity: 0.7;
}
.tools-button {
  position: absolute;
  right: 0;
  top: 50%;
  transform: translateY(-50%);
  display: grid;
  place-items: center;
  width: 34px;
  height: 34px;
  padding: 0;
  border-radius: 10px;
}
.tools-button svg {
  width: 20px;
  height: 20px;
}
.title h2 {
  margin: 0;
  letter-spacing: 0.45em;
  font-weight: 500;
}
.mark {
  font-size: 1.4rem;
  color: var(--accent, #b7a6ff);
}
.problem {
  margin: 0;
  color: var(--danger, #ff9b9b);
}
.note {
  margin: 0;
  opacity: 0.85;
}
.stage {
  flex: 1;
  display: grid;
  grid-template-columns: minmax(0, 2.2fr) minmax(300px, 1fr);
  gap: 12px;
  min-height: 0;
}
.canvas,
.chat,
.dock {
  border: 1px solid color-mix(in srgb, var(--accent, #b7a6ff) 45%, transparent);
  border-radius: 14px;
  background: var(--panel-background, rgba(24, 18, 52, 0.72));
  backdrop-filter: blur(8px);
}
.canvas {
  position: relative;
  display: flex;
  flex-direction: column;
  min-height: 0;
  padding: 10px;
  gap: 8px;
}
.chat {
  position: relative;
  min-height: 0;
  overflow: hidden;
}
.picture {
  position: relative;
  flex: 1;
  min-height: 240px;
  border-radius: 10px;
  overflow: hidden;
  background: rgba(0, 0, 0, 0.25);
}
.picture img {
  width: 100%;
  height: 100%;
  object-fit: contain;
}
.empty {
  display: grid;
  place-items: center;
  height: 100%;
  text-align: center;
  padding: 20px;
  opacity: 0.8;
}
.tag {
  position: absolute;
  transform: translate(-50%, -50%);
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px 10px;
  border-radius: 10px;
  border: 1px solid color-mix(in srgb, var(--accent, #b7a6ff) 70%, transparent);
  background: rgba(30, 22, 70, 0.85);
  white-space: nowrap;
}
.tag.found {
  border-color: #e9c46a;
}
.tag.locked {
  box-shadow: 0 0 12px rgba(233, 196, 106, 0.55);
}
.tag small {
  opacity: 0.75;
}
.canvas-bar {
  display: flex;
  gap: 8px;
  align-items: center;
}
.spacer {
  flex: 1;
}
.details {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.9rem;
}
.details th,
.details td {
  padding: 4px 8px;
  text-align: left;
  border-bottom: 1px solid rgba(255, 255, 255, 0.08);
}
.first-use {
  margin: auto;
  max-width: 460px;
  text-align: center;
  display: grid;
  gap: 10px;
}
.dock {
  display: flex;
  align-items: center;
  gap: 18px;
  padding: 10px 14px;
  overflow-x: auto;
}
.games {
  display: flex;
  gap: 12px;
  align-items: center;
  flex: 1;
}
.game {
  display: grid;
  justify-items: center;
  gap: 4px;
  background: none;
  border: none;
  color: inherit;
  cursor: pointer;
}
.game .initial {
  display: grid;
  place-items: center;
  width: 54px;
  height: 54px;
  border-radius: 50%;
  border: 2px solid color-mix(in srgb, var(--accent, #b7a6ff) 50%, transparent);
  font-size: 1.3rem;
}
.game.on .initial {
  border-color: var(--accent, #b7a6ff);
  box-shadow: 0 0 14px color-mix(in srgb, var(--accent, #b7a6ff) 60%, transparent);
}
.game.refused {
  opacity: 0.45;
}
.remembered {
  font-size: 0.7rem;
  opacity: 0.7;
}
.recipes {
  display: flex;
  gap: 10px;
  border-left: 1px solid rgba(255, 255, 255, 0.12);
  padding-left: 16px;
}
.recipe {
  border-radius: 999px;
  padding: 8px 16px;
}
.recipe.proposed {
  display: flex;
  gap: 6px;
  align-items: center;
}
/* the Forge's chooser: glass cards over the canvas column; the chat and the dock stay as they are */
.chooser {
  position: absolute;
  inset: 0;
  z-index: 2;
  display: flex;
  flex-direction: column;
  gap: 14px;
  padding: 18px;
  border-radius: 14px;
  background: color-mix(in srgb, var(--panel-background, rgba(24, 18, 52, 0.92)) 92%, transparent);
  backdrop-filter: blur(14px);
  overflow: auto;
}
.chooser-head {
  display: flex;
  align-items: baseline;
  justify-content: space-between;
}
.chooser-head h3 {
  margin: 0;
  letter-spacing: 0.25em;
  font-weight: 500;
}
.cards {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
  gap: 14px;
}
.tool-card {
  display: grid;
  justify-items: start;
  gap: 6px;
  padding: 16px;
  text-align: left;
  color: inherit;
  border-radius: 14px;
  border: 1px solid color-mix(in srgb, var(--accent, #b7a6ff) 35%, transparent);
  background: linear-gradient(160deg, rgba(120, 96, 220, 0.22), rgba(40, 28, 90, 0.35));
  backdrop-filter: blur(10px);
  cursor: pointer;
  transition: border-color 0.2s, box-shadow 0.2s, transform 0.2s;
}
.tool-card:hover:not(:disabled) {
  border-color: var(--accent, #b7a6ff);
  transform: translateY(-1px);
}
.tool-card.last {
  border-color: var(--accent, #b7a6ff);
  box-shadow: 0 0 18px color-mix(in srgb, var(--accent, #b7a6ff) 55%, transparent), inset 0 0 12px color-mix(in srgb, var(--accent, #b7a6ff) 18%, transparent);
}
.tool-card.dim {
  opacity: 0.5;
  cursor: not-allowed;
  box-shadow: none;
}
.tool-icon {
  width: 44px;
  height: 44px;
  color: var(--accent, #b7a6ff);
}
.tool-icon .spark,
.tool-icon .spark-text {
  fill: #fff4c8;
}
.tool-card strong {
  font-size: 1.05rem;
}
.tool-line {
  opacity: 0.85;
}
.kind {
  font-size: 0.75rem;
  letter-spacing: 0.08em;
  color: var(--accent, #b7a6ff);
}
.needs {
  font-size: 0.75rem;
  color: var(--danger, #ff9b9b);
}
/* the Translate Lens's LINES column */
.lines-head {
  display: flex;
  align-items: baseline;
  gap: 12px;
}
.lines-head h3 {
  margin: 0;
  letter-spacing: 0.3em;
  font-weight: 500;
}
.lines-head small {
  opacity: 0.7;
}
.lines {
  flex: 1;
  min-height: 0;
  margin: 0;
  padding: 0;
  list-style: none;
  overflow: auto;
  display: grid;
  align-content: start;
  gap: 8px;
}
.lines li {
  display: grid;
  gap: 2px;
  padding: 8px 12px;
  border-radius: 10px;
  border: 1px solid color-mix(in srgb, var(--accent, #b7a6ff) 25%, transparent);
  background: rgba(0, 0, 0, 0.2);
}
.original {
  opacity: 0.65;
}
.translated {
  font-size: 1.02rem;
}
.lines-empty {
  flex: 1;
  height: auto;
}
.auto {
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
.key-hint {
  margin: 0;
  font-size: 0.85rem;
  opacity: 0.7;
}
.link {
  background: none;
  border: none;
  color: var(--accent, #b7a6ff);
  text-decoration: underline;
  cursor: pointer;
}
</style>
