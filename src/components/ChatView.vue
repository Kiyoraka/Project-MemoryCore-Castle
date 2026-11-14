<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { chatHistory, chatPoll, queuedMessageAction, queuedMessages, sendMessage } from '../api/castle.js'
import { localSend, mergeEntries, resetEntries } from './ticks.js'
import { readChoices } from './choices.js'
import { firstHeading } from './markdown.js'
import { MODES, readContext, readMode, SHIFT_TAB } from './modes.js'
import { useChatMedia } from './useChatMedia.js'
import ChatComposer from './ChatComposer.vue'
import ChatEntries from './ChatEntries.vue'
import ImageViewer from './ImageViewer.vue'
import PlanViewer from './PlanViewer.vue'
import { worldSignals } from '../world3d/signals.js'

// CHAT: a session as a conversation between two people. This file reads the session's record and
// the Log screen and holds the state; the conversation and the choice card (ChatEntries) and the
// box (ChatComposer) only show and ask.
const props = defineProps({
  // Whether the session is running.
  running: { type: Boolean, default: false },
  // Whether the Chat tab is the one showing (answer buttons and focus). Reading goes on on any
  // tab while Castle is visible, so the companion's voice can play wherever the member is in Castle.
  visible: { type: Boolean, default: false },
  // Sends text into the same session, as if typed in the Log.
  write: { type: Function, required: true },
  // The last lines of the Log screen, for the answer buttons.
  readScreen: { type: Function, default: () => [] },
  // undefined for the main session, 'story' for the STORY room's, 'dev:<slot>' for a DEV tab's
  room: { type: String, default: undefined },
  // the companion's name, for labels
  companion: { type: String, default: 'Your companion' },
  // 'claude' or 'codex': answer buttons, send-now and the mode pills are Claude Code's; Codex asks in the Log
  cli: { type: String, default: 'claude' },
})
// busy: the companion is working or not (the typing bubble)
// context: the context window as Castle's status line says it ({ percent, tokens }), when it
// changes, and null once the session ends
const emit = defineEmits(['show-log', 'busy', 'context'])

const POLL_MS = 1000
// The typing bubble gives up after this long without a new step (a long build keeps it).
const TYPING_FOR_MS = 10 * 60 * 1000

const entries = ref([])
const status = ref('asleep')
const now = ref(Date.now())
const conversation = ref(null)
const composer = ref(null)
const { media, durations, playing, loadMedia, loadPath, playVoice, toggleVoice, arrive, stop } = useChatMedia(() => props.room)
let timer
let menuTimer
let clockTimer

// ---------- Messages waiting for the companion's current work (main room) ----------

const pendingMessages = ref([])
const sendProblem = ref('')
async function queueAction(id, retry) {
  try {
    await queuedMessageAction(id, retry)
    pendingMessages.value = await queuedMessages()
  } catch (e) {
    sendProblem.value = String(e)
  }
}

onMounted(async () => {
  try {
    entries.value = resetEntries(entries.value, await chatHistory(props.room))
  } catch {
    // The first poll fills it.
  }
})

// ---------- The card: a choice asked in the Log ----------

// A choice menu read from the Log screen (a permission prompt or a question), or null.
const menu = ref(null)
// Codex is asking for an approval on the Log screen (its prompts are answered there).
const codexAsking = ref(false)
// Words Codex shows while it waits for an approval.
const CODEX_ASK = /allow command|approve|apply (these )?changes|yes, proceed|\(y\/n\)|\[y\/n\]/i
const isClaude = computed(() => props.cli !== 'codex')

function readMenu() {
  if (!props.visible || !props.running) {
    menu.value = null
    return
  }
  if (!isClaude.value) {
    menu.value = null
    codexAsking.value = CODEX_ASK.test(props.readScreen(15).join('\n'))
    return
  }
  const lines = props.readScreen(40)
  menu.value = readChoices(lines)
  // the mode, from the same screen: the lit pill follows Claude Code's own footer
  if (!modeBusy) mode.value = readMode(lines)
  // the context, from Castle's status line on the same screen; the last value stays while it is hidden
  const context = readContext(lines)
  if (context && (context.percent !== lastContext?.percent || context.tokens !== lastContext?.tokens)) {
    lastContext = context
    emit('context', context)
  }
}
let lastContext = null

async function choose(number) {
  menu.value = null
  try {
    await props.write(String(number))
  } catch {
    // The menu comes back on the next read.
  }
}

// ---------- The mode (manual / accept edits / plan / auto, Shift+Tab in Claude Code) ----------

const mode = ref(null)
const modeProblem = ref('')
let modeBusy = false
const pause = (ms) => new Promise((done) => setTimeout(done, ms))

/** Presses Shift+Tab, reading the footer after each press, until it shows the mode chosen. */
async function setMode(target) {
  if (!props.running || menu.value || modeBusy || mode.value === target) return
  modeBusy = true
  modeProblem.value = ''
  try {
    for (let press = 0; press < MODES.length; press += 1) {
      await props.write(SHIFT_TAB)
      await pause(300)
      const shown = readMode(props.readScreen(40))
      mode.value = shown
      if (shown === target) return
    }
    modeProblem.value = 'The mode did not change - open Log to see.'
  } catch {
    modeProblem.value = 'The mode could not be changed.'
  } finally {
    modeBusy = false
  }
}

// The session ended: no mode to show, and the context meter goes away.
watch(
  () => props.running,
  (on) => {
    if (on) return
    mode.value = null
    if (lastContext) {
      lastContext = null
      emit('context', null)
    }
  },
)

// ---------- The conversation ----------

// A conversation between two people: the companion's tool steps stay in Log, and while it works Chat shows
// only a typing bubble. Pictures made one after another sit side by side.
const items = computed(() => {
  const out = []
  const plansSeen = new Set()
  for (const entry of entries.value) {
    const last = out[out.length - 1]
    if (entry.role === 'work') continue
    if (entry.role === 'plan') {
      // The same plan asked, then approved, is one card.
      if (!plansSeen.has(entry.text)) out.push({ kind: 'plan', id: entry.id, entry, ...planFace(entry.text) })
      plansSeen.add(entry.text)
    } else if (entry.role === 'picture' && last?.kind === 'pictures') last.list.push(entry)
    else if (entry.role === 'picture') out.push({ kind: 'pictures', id: entry.id, list: [entry] })
    else if (entry.role === 'voice') out.push({ kind: 'voice', id: entry.id, entry })
    else out.push({ kind: 'message', id: entry.id, entry })
  }
  return out
})

// The companion is typing while its newest entry is a tool step: it is working toward its next words.
const typing = computed(() => {
  if (!props.running) return false
  const newest = entries.value[entries.value.length - 1]
  if (newest?.role !== 'work') return false
  const at = Date.parse(newest.at ?? '')
  return Number.isNaN(at) || now.value - at < TYPING_FOR_MS
})

// The WORLD on the other screens knows when the companion works and when it rests - in the main
// room only: a DEV tab's session is work in a project, not her presence in the WORLD.
const signalsWorld = () => !String(props.room ?? '').startsWith('dev:')
watch(
  typing,
  (on) => {
    if (signalsWorld()) worldSignals.busy = on
    emit('busy', on)
  },
  { immediate: true },
)
watch(() => props.running, (on) => { if (signalsWorld()) worldSignals.asleep = !on }, { immediate: true })

// A plan card shows only its title and how many phases; the reader holds the whole plan.
function planFace(text) {
  const title = firstHeading(text) || 'Plan'
  const phases = (text.match(/^#{2,3}\s+Phase\b/gim) || []).length
  return { title, phases }
}

const planOpen = ref(null)

// ---------- Pictures in the viewer ----------

const viewerStart = ref(null)
const fileName = (path) => path.split(/[\\/]/).pop()

// Every picture in this chat in order - the ones the companion made and the ones the member shared - for the viewer.
const allPictures = computed(() =>
  entries.value.flatMap((e) => {
    if (e.role === 'picture') return [{ path: e.path, name: e.text, url: media[e.path] }]
    if (e.role === 'member' && e.pictures?.length) return e.pictures.map((p) => ({ path: p, name: fileName(p), url: media[p] }))
    return []
  }),
)

function openPicture(path) {
  const at = allPictures.value.findIndex((p) => p.path === path)
  viewerStart.value = at < 0 ? 0 : at
}

// ---------- Reading the record ----------

let lastPollAt = 0

async function poll() {
  if (!props.room) {
    try {
      pendingMessages.value = await queuedMessages()
    } catch {
      // the list stays as it was
    }
  }
  if (document.visibilityState !== 'visible') return
  // A voice plays by itself only when it arrives while Castle was in view; one made while
  // Castle was hidden arrives in a catch-up read and waits for "play".
  const live = Date.now() - lastPollAt < POLL_MS * 3.5
  lastPollAt = Date.now()
  let answer
  try {
    answer = await chatPoll(props.room)
  } catch {
    return
  }
  status.value = answer.status
  answer.entries.filter((e) => e.path).forEach(loadMedia)
  answer.entries.filter((e) => e.pictures?.length).forEach((e) => e.pictures.forEach(loadPath))
  // the STORY stage reads the story's replies too (its fallback when no beat card is written)
  if (props.room === 'story' && !answer.reset) {
    answer.entries.filter((e) => e.role === 'companion' && e.text).forEach((e) => window.dispatchEvent(new CustomEvent('castle-story-reply', { detail: e.text })))
  }
  if (!answer.reset && live) {
    const newest = [...answer.entries].reverse().find((e) => e.role === 'voice')
    if (newest) arrive(newest)
  }
  if (answer.reset) {
    entries.value = resetEntries(entries.value, answer.entries)
  } else if (answer.entries.length) {
    const follow = conversation.value?.nearBottom() ?? true
    entries.value = mergeEntries(entries.value, answer.entries)
    if (!follow) return
  } else {
    return
  }
  await nextTick()
  conversation.value?.toBottom()
}

// ---------- The member's words ----------

const placeholder = computed(() => (props.running ? `Talk to ${props.companion}` : `${props.companion} is resting`))

/** Sends the member's words; true when they went in (the box then empties). */
async function send(text) {
  if (!props.running) return false
  // Several lines go in as one paste, so a new line never sends the message early.
  let sent
  try {
    sent = await sendMessage(text, props.room)
  } catch (reason) {
    sendProblem.value = String(reason)
    // The words stay in the box to try again.
    return false
  }
  if (signalsWorld()) worldSignals.sentAt = Date.now() // in the WORLD the crystal brightens
  try {
    // the member's bubble at once, one grey tick until the companion's record holds it
    if (sent.queued) pendingMessages.value = await queuedMessages()
    else entries.value = [...entries.value, localSend(text)]
    sendProblem.value = ''
  } catch (reason) {
    sendProblem.value = String(reason)
  }
  await nextTick()
  conversation.value?.toBottom()
  return true
}

// A queued message goes in now instead of after the companion's current step (Claude Code's Ctrl+X Ctrl+S).
async function sendNow() {
  try {
    await props.write('\u0018\u0013')
  } catch {
    // it stays queued
  }
}

watch(
  () => props.visible,
  (shown) => {
    if (shown) {
      poll()
      nextTick(() => composer.value?.focus())
    }
  },
)

onMounted(() => {
  poll()
  timer = setInterval(poll, POLL_MS)
  menuTimer = setInterval(readMenu, 700)
  clockTimer = setInterval(() => (now.value = Date.now()), 5000)
  document.addEventListener('visibilitychange', poll)
})

onBeforeUnmount(() => {
  clearInterval(timer)
  clearInterval(menuTimer)
  clearInterval(clockTimer)
  document.removeEventListener('visibilitychange', poll)
  stop()
})

/** Files dropped on Chat: their paths go into the box. */
const addPaths = (paths) => composer.value?.addPaths(paths)

defineExpose({ poll, addPaths })
</script>

<template>
  <div class="chat">
    <div v-if="pendingMessages.length" class="pending-messages" aria-live="polite">
      <p v-for="message in pendingMessages" :key="message.id">
        <strong>{{ message.state === 'queued' ? 'Queued until work finishes' : 'Check chat before retrying this interrupted message' }}</strong>: {{ message.text }}
        <button v-if="message.state !== 'queued'" @click="queueAction(message.id, true)">Retry</button>
        <button @click="queueAction(message.id, false)">Cancel</button>
      </p>
    </div>
    <p v-if="sendProblem" role="alert">{{ sendProblem }}</p>

    <ChatEntries
      ref="conversation"
      :status="status"
      :items="items"
      :typing="typing"
      :media="media"
      :durations="durations"
      :playing="playing"
      :menu="menu"
      :codex-asking="codexAsking"
      :companion="companion"
      :cli="cli"
      @show-log="emit('show-log')"
      @send-now="sendNow"
      @open-plan="planOpen = $event"
      @open-picture="openPicture"
      @toggle-voice="toggleVoice"
      @play-voice="playVoice"
      @choose="choose"
      @choose-designs="send"
    />

    <ImageViewer v-if="viewerStart !== null" :pictures="allPictures" :start="viewerStart" @close="viewerStart = null" />
    <PlanViewer v-if="planOpen !== null" :plan="planOpen" @close="planOpen = null" />

    <ChatComposer ref="composer" :running="running" :placeholder="placeholder" :companion="companion" :media="media" :load-path="loadPath" :send="send" />

    <div v-if="running && isClaude" class="modes" role="group" aria-label="Claude Code's mode">
      <span class="modes-label">MODE</span>
      <button
        v-for="m in MODES"
        :key="m.id"
        class="mode"
        :class="{ on: mode === m.id }"
        :disabled="!!menu"
        :title="menu ? 'Answer the question first' : `Switch to ${m.label}`"
        @click="setMode(m.id)"
      >
        {{ m.label }}
      </button>
      <span class="modes-hint">{{ modeProblem || 'shift+tab cycles' }}</span>
    </div>
  </div>
</template>

<style scoped>
.chat {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
  min-height: 0;
  padding-right: 10px;
}

button {
  font: inherit;
  cursor: pointer;
}

/* The mode, under the message box: the lit pill is what Claude Code's own footer says. */
.modes {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  padding: 6px 2px 0;
  font-size: 12px;
}

.modes-label {
  letter-spacing: 0.16em;
  font-size: 10.5px;
  color: var(--text-dim);
}

.mode {
  padding: 3px 11px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: none;
  color: inherit;
  font: inherit;
  cursor: pointer;
}

.mode.on {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 26%, transparent);
}

.mode:disabled {
  opacity: 0.45;
  cursor: default;
}

.mode:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

.modes-hint {
  margin-left: auto;
  color: var(--text-dim);
  font-size: 11px;
}
</style>
