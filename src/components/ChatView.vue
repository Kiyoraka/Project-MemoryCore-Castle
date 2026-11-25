<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { chatHistory, chatMediaOpen, chatPlanFile, chatPoll, chatPromptHolds, claudeVoice, getSettings, onPhoneReaction, phoneReactions, queuedMessageAction, queuedMessages, sendMessage } from '../api/castle.js'
import { clearNotSent, localSend, markNotSent, mergeEntries, resetEntries, stillSending } from './ticks.js'
import { isMicShortcut, micOffer, voiceMic } from './voicehold.js'
import { isMac } from '../platform.js'
import { isOwnAnswer, readChoices, tellOf } from './choices.js'
import { buildItems, isMarker, openPlanFile, placeReactions, withReaction } from './chatItems.js'
import { MODES, readContext, readMode, SHIFT_TAB } from './modes.js'
import { useChatMedia } from './useChatMedia.js'
import { enterDelay, typeLine } from './typeline.js'
import { pasted } from '../views/dev/devtabs.js'
import ChatChoices from './ChatChoices.vue'
import ChatComposer from './ChatComposer.vue'
import ChatEntries from './ChatEntries.vue'
import ImageViewer from './ImageViewer.vue'
import PlanViewer from './PlanViewer.vue'
import { worldSignals } from '../world3d/signals.js'
import { readsClaudeScreen, readsCodexAsk } from './providers.js'

// CHAT: a session as a conversation between two people. This file reads the session's record and
// the Log screen and holds the state; the conversation (ChatEntries), the choice card (ChatChoices)
// and the box (ChatComposer) only show and ask.
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
  // a provider id (providers.js): answer buttons, send-now and the mode pills are Claude Code's;
  // Codex's approval prompt is noticed; any other provider has the plain box, approvals in the Log
  cli: { type: String, default: 'claude' },
  // Bumped when Settings are saved: the mic's shortcut is read again
  settingsStamp: { type: Number, default: 0 },
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
const isClaude = computed(() => readsClaudeScreen(props.cli))

function readMenu() {
  if (!props.visible || !props.running) {
    menu.value = null
    return
  }
  if (!isClaude.value) {
    menu.value = null
    codexAsking.value = readsCodexAsk(props.cli) && CODEX_ASK.test(props.readScreen(15).join('\n'))
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

// The option whose text field the box stands in for while it is up ("Type something.", "Tell
// Claude what to change"); Claude Code only.
const tellOption = computed(() => (isClaude.value ? tellOf(menu.value) : undefined))

async function choose(number) {
  // an option with no number on screen (the folder-trust question) carries the keys that reach it
  const keys = menu.value?.options?.find((option) => option.number === number)?.keys
  menu.value = null
  try {
    await props.write(keys ?? String(number))
  } catch {
    // The menu comes back on the next read.
  }
}

// The Submit row of a question with several answers: the keys readChoices worked out (the next
// tab, or the arrows down to the row and Enter).
async function submitChoices() {
  const keys = menu.value?.submit?.keys
  if (!keys) return
  menu.value = null
  try {
    await props.write(keys)
  } catch {
    // The menu comes back on the next read.
  }
}

/** Answers the question showing now from the box: picks the free-text option, then types the words. */
async function answerFromBox(option, text) {
  menu.value = null
  try {
    await props.write(String(option.number))
    // give the terminal a moment to open its field
    await new Promise((resolve) => setTimeout(resolve, 400))
    await typeLine(props.write, pasted(text))
  } catch (reason) {
    sendProblem.value = String(reason)
    return false
  }
  entries.value = [...entries.value, localSend(text)]
  sendProblem.value = ''
  await nextTick()
  conversation.value?.toBottom()
  return true
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
// only a typing bubble. Pictures made one after another sit side by side; the same plan asked, then
// approved, is one card; the plan read from its file while the record is late sits at the bottom
// only while that plan is open (chatItems.js).
// The member's hearts from the phone sit on the companion's bubbles (main room only; the phone
// talks to the main session).
const reactions = ref([])
const items = computed(() => {
  const built = buildItems(entries.value, waitingPlan.value)
  return props.room ? built : placeReactions(built, reactions.value)
})
let stopReactions = null

// The companion is typing while its newest entry is a tool step: it is working toward its next words.
const typing = computed(() => {
  if (!props.running) return false
  const newest = entries.value.findLast((entry) => !isMarker(entry))
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

const planOpen = ref(null)

// Open / Open folder under a film or a song, as the picture viewer does.
async function openMedia({ path, folder }) {
  try {
    await chatMediaOpen(path, folder, props.room)
  } catch {
    // The system said no; the row stays as it was.
  }
}

// Claude Code may write the "present the plan" step into its record only after the member has
// answered, so the approve question can be on screen with no plan card to read. The record does name
// the plan file earlier; when a plan question is up and no plan has come since, Chat reads that file.
const PLAN_QUESTION = /auto mode|manually approve|keep planning/i
const planAsked = computed(() => !!menu.value?.options.some((option) => PLAN_QUESTION.test(option.label)))
// The newest plan file, unless a plan card or the end of plan mode already followed it.
const planFile = computed(() => openPlanFile(entries.value))
const waitingPlan = ref(null)
watch([planAsked, planFile], async ([asked, path]) => {
  if (!asked || !path) return
  try {
    const text = await chatPlanFile(path)
    if (text) waitingPlan.value = { path, text, at: new Date().toISOString() }
  } catch {
    // No card rather than a wrong one; the Log still shows the plan.
  }
})

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

const placeholder = computed(() => {
  if (!props.running) return `${props.companion} is resting`
  if (tellOption.value) return isOwnAnswer(tellOption.value) ? 'Your own answer...' : `Tell ${props.companion} what to change...`
  return `Talk to ${props.companion}`
})

/** Sends the member's words; true when they went in (the box then empties). */
async function send(text) {
  if (!props.running) return false
  // An answer must reach the question showing now, never wait in the queue behind it.
  if (tellOption.value) return answerFromBox(tellOption.value, text)
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
    else {
      const bubble = localSend(text)
      entries.value = [...entries.value, bubble]
      watchLanding(bubble.id, text)
    }
    sendProblem.value = ''
  } catch (reason) {
    sendProblem.value = String(reason)
  }
  await nextTick()
  conversation.value?.toBottom()
  return true
}

// ---------- A message that really got through ----------

// Claude Code can take the words and their Enter as one paste (or be busy taking in a picture) and
// keep the words in its box unsent. 3 s after a direct send and its Enter (which waits longer for
// pictures), words still in the box get one more Enter; still there 3 s later, the bubble says
// "Not sent · Send again" instead of a tick.
const LANDING_MS = 3000

async function promptHolds(text) {
  if (!isClaude.value) return false
  try {
    return await chatPromptHolds(props.room, text)
  } catch {
    return false
  }
}

function watchLanding(id, text) {
  setTimeout(async () => {
    if (!stillSending(entries.value, id) || !(await promptHolds(text))) return
    try {
      await props.write('\r')
    } catch {
      // the check below still tells
    }
    await pause(LANDING_MS)
    if (stillSending(entries.value, id) && (await promptHolds(text))) entries.value = markNotSent(entries.value, id)
  }, LANDING_MS + enterDelay(text))
}

/** Send again on a Not sent bubble: Enter when the words still sit in the box, else typed again. */
async function resend(entry) {
  if (!props.running) return
  try {
    if (await promptHolds(entry.text)) {
      await props.write('\r')
    } else {
      const sent = await sendMessage(entry.text, props.room)
      if (sent.queued) {
        // it waits in the queue now and shows there instead
        entries.value = entries.value.filter((m) => m.id !== entry.id)
        pendingMessages.value = await queuedMessages()
        return
      }
    }
    entries.value = clearNotSent(entries.value, entry.id)
    watchLanding(entry.id, entry.text)
    sendProblem.value = ''
  } catch (reason) {
    // it stays Not sent
    sendProblem.value = String(reason)
  }
}

// A queued message goes in now instead of after the companion's current step (Claude Code's Ctrl+X Ctrl+S).
async function sendNow() {
  try {
    await props.write('\u0018\u0013')
  } catch {
    // it stays queued
  }
}

// ---------- The mic (Claude Code's own voice mode) ----------

// The member's Claude Code voice: { enabled, mode }; undefined until read, null when it cannot be read.
const voice = ref(undefined)
let voiceTimer
async function readVoice() {
  try {
    voice.value = await claudeVoice()
  } catch {
    voice.value = null
  }
}

// The mic's shortcut as chosen in Settings > Chat > Mic (null = ⌘⇧M / Ctrl+Shift+M); undefined until
// read, and the 🎙 waits for it, so it never shows the default keys first.
const micShortcut = ref(undefined)
async function readMicShortcut() {
  try {
    micShortcut.value = (await getSettings())?.settings?.micShortcut ?? null
  } catch {
    // keeps the one it has; the first time, the default
    if (micShortcut.value === undefined) micShortcut.value = null
  }
}
watch(() => props.settingsStamp, readMicShortcut)

// The mic holds Space in this session while it is on, wherever it runs (HOME, STORY or a DEV tab):
// `write` is the panel's own session, and the words are read back off the same panel's Log screen.
const micHold = voiceMic({
  write: (data) => props.write(data),
  readScreen: (count) => props.readScreen(count),
  readTyped: (count) => props.readScreen(count, { typed: true }),
})

// Shown only while Claude Code's voice is on (hold mode, which records while Space is held). When Castle cannot
// tell, it shows with a hint to turn voice on first.
const mic = computed(() => {
  const offer = micOffer({ running: props.running, claude: isClaude.value, voice: voice.value, shortcut: micShortcut.value })
  return offer && { ...offer, hold: micHold }
})

watch(
  () => props.visible,
  (shown) => {
    if (shown) {
      poll()
      readVoice()
      readMicShortcut()
      nextTick(() => composer.value?.focus())
    }
  },
)

// The mic's shortcut (⌘⇧M / Ctrl+Shift+M unless changed in Settings) turns the mic on or off from
// anywhere in Chat, the message box too; Esc while it is on cancels. Only these keys: every other
// one goes on as before. Keys pressed where Castle records a shortcut ([data-takes-keys]) are left alone.
function micKeys(event) {
  if (!props.visible || !mic.value || event.target?.closest?.('[data-takes-keys]')) return
  if (isMicShortcut(event, isMac, micShortcut.value)) {
    event.preventDefault()
    composer.value?.micToggle()
  } else if (event.key === 'Escape' && composer.value?.micOn()) {
    event.preventDefault()
    event.stopPropagation()
    composer.value.micCancel()
  }
}

onMounted(() => {
  poll()
  readVoice()
  readMicShortcut()
  window.addEventListener('keydown', micKeys, true)
  // /voice in the Log turns the mic on or off within a few seconds
  voiceTimer = setInterval(() => props.visible && readVoice(), 5000)
  timer = setInterval(poll, POLL_MS)
  menuTimer = setInterval(readMenu, 700)
  clockTimer = setInterval(() => (now.value = Date.now()), 5000)
  document.addEventListener('visibilitychange', poll)
  if (!props.room) {
    phoneReactions().then((kept) => (reactions.value = kept ?? [])).catch(() => {})
    onPhoneReaction((reaction) => (reactions.value = withReaction(reactions.value, reaction))).then((stop) => (stopReactions = stop))
  }
})

onBeforeUnmount(() => {
  clearInterval(timer)
  clearInterval(voiceTimer)
  window.removeEventListener('keydown', micKeys, true)
  clearInterval(menuTimer)
  clearInterval(clockTimer)
  document.removeEventListener('visibilitychange', poll)
  stopReactions?.()
  stop()
})

/** Files dropped on Chat: their paths go into the box. */
const addPaths = (paths) => composer.value?.addPaths(paths)
const addText = (text) => composer.value?.addText(text)

defineExpose({ poll, addPaths, addText })
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
      :codex-asking="codexAsking"
      :companion="companion"
      :cli="cli"
      @show-log="emit('show-log')"
      @send-now="sendNow"
      @resend="resend"
      @open-plan="planOpen = $event"
      @open-picture="openPicture"
      @toggle-voice="toggleVoice"
      @play-voice="playVoice"
      @choose-designs="send"
      @open-media="openMedia"
      @media-play="stop"
    />

    <ChatChoices v-if="menu" :menu="menu" :companion="companion" @choose="choose" @submit="submitChoices" @show-log="emit('show-log')" />

    <ImageViewer v-if="viewerStart !== null" :pictures="allPictures" :start="viewerStart" @close="viewerStart = null" />
    <PlanViewer v-if="planOpen !== null" :plan="planOpen" @close="planOpen = null" />

    <ChatComposer ref="composer" :running="running" :placeholder="placeholder" :companion="companion" :media="media" :load-path="loadPath" :send="send" :mic="mic" :visible="visible" />

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
