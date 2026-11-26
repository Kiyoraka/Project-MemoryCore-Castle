<script setup>
// WORLDSMITH (S-1126-7 / S-1126-9, his pick worldsmith-9): the AI cheat engine for the member's own
// offline single-player games. The newest screenshot fills the canvas with every value the companion
// found pinned on it (a Lock on each); the room's own session is the chat on the right; the games and
// the recipes sit in the dock below. The small memory helper is fetched only when the member presses
// Get it, starts only while the room is open, and lets the game go when the room closes.
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import TerminalPanel from '../components/TerminalPanel.vue'
import {
  onWorldsmithInstall,
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
} from '../api/castle.js'
import { helperWord, lockCall, readGames, readState, recipeWord, shotWord, stepsLine, tagPlaces } from './worldsmith/worldsmith.js'

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
let pollTimer = 0
const stops = []

const tags = computed(() => tagPlaces(state.value.labels, memory.value.values))
const keptRecipes = computed(() => memory.value.recipes.filter((r) => r.state === 'kept'))
const proposed = computed(() => memory.value.recipes.filter((r) => r.state === 'proposed'))
const helperLine = computed(() => helperWord(helper.value))
const shotSrc = computed(() => (shot.value ? convertFileSrc(shot.value) : ''))

const say = (reason) => (problem.value = String(reason ?? ''))

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
  await prepare()
})

onBeforeUnmount(() => {
  stopPolling()
  stops.forEach((stop) => stop())
  // the room closed: the game is let go, every frozen value stops, the session ends
  worldsmithStop().catch(() => {})
  worldsmithClose().catch(() => {})
})

// the companion remembers values and pins as it works: the canvas and the recipes follow
watch(() => state.value.labels.map((l) => `${l.label}:${l.steps.length}`).join(','), refreshMemory)
watch(() => props.visible, (on) => on && helper.value?.installed && loadGames())
</script>

<template>
  <div class="worldsmith">
    <header class="title">
      <span class="mark" aria-hidden="true">✦</span>
      <h2>WORLDSMITH</h2>
    </header>

    <p v-if="problem" class="problem" role="alert">{{ problem }}</p>
    <p v-if="note" class="note">{{ note }}</p>

    <div class="stage">
      <section class="canvas" aria-label="The newest screenshot with the values found">
        <div v-if="helper && !helper.installed" class="first-use">
          <h3>A small helper first</h3>
          <p>{{ helperLine }}</p>
          <button v-if="helper.windows && helper.released" class="primary" :disabled="getting" @click="getHelper">{{ getting ? 'Getting it...' : 'Get it' }}</button>
          <progress v-if="progress" :value="progress.done" :max="progress.total || 1"></progress>
        </div>
        <template v-else>
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
      <div class="recipes" aria-label="Recipes">
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
  display: grid;
  grid-template-rows: auto auto auto 1fr auto;
  gap: 10px;
  height: 100%;
  padding: 14px 18px;
  box-sizing: border-box;
}
.title {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 12px;
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
.link {
  background: none;
  border: none;
  color: var(--accent, #b7a6ff);
  text-decoration: underline;
  cursor: pointer;
}
</style>
