<script setup>
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import { Unicode11Addon } from '@xterm/addon-unicode11'
import '@xterm/xterm/css/xterm.css'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import ChatView from './ChatView.vue'
import ProfileView from '../views/profile/ProfileView.vue'
import { isTerminalReply } from './terminalreply.js'
import QuickActions from './QuickActions.vue'
import {
  clipboardCopy,
  clipboardPaste,
  devChatShown,
  devResize,
  devSnapshot,
  devState,
  devWake,
  devWrite,
  onDevExit,
  onDevOutput,
  onStoryExit,
  onStoryOutput,
  onTerminalExit,
  onTerminalOutput,
  onWakeRequest,
  takeAutoWake,
  terminalResize,
  terminalStart,
  terminalState,
  terminalWrite,
} from '../api/castle.js'
import { devRoom, sessionView, tabForSlot } from '../views/dev/devtabs.js'

const props = defineProps({
  // Changes whenever Settings saves, so the session status is read again.
  settingsStamp: { type: Number, default: 0 },
  // undefined for the companion's main session, 'story' for the STORY room's
  room: { type: String, default: undefined },
  // the companion's name, for the buttons and messages
  companion: { type: String, default: 'Your companion' },
  // 'claude' or 'codex'
  cli: { type: String, default: 'claude' },
  // A DEV tab's slot (0-7): the panel then serves that tab's session, sits inside the tab (no
  // header, quick actions or drag handle) and its CHAT / LOG is chosen by `showLog`.
  devSlot: { type: Number, default: null },
  // (DEV) the tab is the one showing: its chat is read only then
  shown: { type: Boolean, default: true },
  // (DEV) the Log shows instead of the chat
  showLog: { type: Boolean, default: false },
})
// (DEV) `view`: 'chat' | 'log' when the panel switched itself; `running`: the session started or
// ended; `state`: a fresh dev state from Wake; `busy`: the companion is working (Chat's typing
// bubble); `context`: the session's context ({ percent, tokens }) when it changes, null once it ends
const emit = defineEmits(['settings', 'setup', 'view', 'running', 'state', 'busy', 'context'])

const isDev = props.devSlot !== null && props.devSlot !== undefined
// HOME's panel has a PROFILE tab (the emotion engine); STORY and DEV tabs do not
const hasProfile = !isDev && props.room !== 'story'
// the room Chat reads: a DEV tab's is routed to its own session (see castle.js)
const chatRoom = isDev ? devRoom(props.devSlot) : props.room

// The one session this panel serves: the companion's (main or STORY) or a DEV tab's.
const session = isDev
  ? {
      async state() {
        return sessionView(tabForSlot((await devState()).tabs, props.devSlot))
      },
      snapshot: () => devSnapshot(props.devSlot),
      async start(cols, rows) {
        const state = await devWake(props.devSlot, cols, rows)
        emit('state', state)
        return sessionView(tabForSlot(state.tabs, props.devSlot))
      },
      write: (data) => devWrite(props.devSlot, data),
      resize: (cols, rows) => devResize(props.devSlot, cols, rows),
      onOutput: (handler) => onDevOutput(props.devSlot, handler),
      onExit: (handler) => onDevExit(props.devSlot, handler),
    }
  : {
      state: () => terminalState(props.room),
      snapshot: null,
      start: (cols, rows, greet) => terminalStart(cols, rows, props.room, greet),
      write: (data) => terminalWrite(data, props.room),
      resize: (cols, rows) => terminalResize(cols, rows, props.room),
      onOutput: props.room === 'story' ? onStoryOutput : onTerminalOutput,
      onExit: props.room === 'story' ? onStoryExit : onTerminalExit,
    }

const WIDTH_KEY = 'castle.panelWidth'
const VIEW_KEY = 'castle.panelView'
const MIN_WIDTH = 25
const MAX_WIDTH = 75

const surface = ref(null)
const panel = ref(null)
const running = ref(false)
let starting = false
const activeProvider = ref(null)
const ended = ref(false)
const status = ref({ ready: false, message: '', fix: null })
const problem = ref('')
const width = ref(readWidth())
// CHAT or LOG under the companion's portrait; the wide button gives the panel the whole screen.
const view = ref(readView())
const expanded = ref(false)
const chat = ref(null)
// A file is being dragged over Castle while this panel shows.
const dropping = ref(false)

const quoted = (path) => (/\s/.test(path) ? `"${path}"` : path)

// Files dropped on Castle land where the member is: in Chat's box (pictures previewed), or typed into
// the Log. Only while this panel is the one showing.
function onDragDrop(event) {
  if (!panel.value || panel.value.offsetParent === null) return
  const drop = event.payload
  if (drop.type === 'enter' || drop.type === 'over') dropping.value = true
  else if (drop.type === 'leave') dropping.value = false
  else if (drop.type === 'drop') {
    dropping.value = false
    const paths = drop.paths ?? []
    if (!paths.length) return
    if (view.value === 'chat') chat.value?.addPaths(paths)
    else if (view.value === 'log' && running.value) term.paste(paths.map(quoted).join(' '))
  }
}

function readView() {
  if (isDev) return props.showLog ? 'log' : 'chat'
  try {
    const saved = localStorage.getItem(VIEW_KEY)
    if (saved === 'chat' || saved === 'log' || (saved === 'profile' && hasProfile)) return saved
  } catch {
    // Storage can be unavailable; Chat is shown.
  }
  return 'chat'
}

function setView(next) {
  view.value = next
  if (isDev) {
    // the DEV tab's own Log button follows
    emit('view', next)
    return
  }
  try {
    localStorage.setItem(VIEW_KEY, next)
  } catch {
    // Not kept; the next start shows Log.
  }
}

let term
let fit
let observer
let fitTimer
const stopListening = []

function readWidth() {
  try {
    const saved = Number(localStorage.getItem(WIDTH_KEY))
    if (saved >= MIN_WIDTH && saved <= MAX_WIDTH) return saved
  } catch {
    // Storage can be unavailable; the theme's width is used.
  }
  return null
}

function cssVar(name) {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim()
}

function show(state) {
  activeProvider.value = state.provider ?? null
  running.value = state.running
  status.value = state.status
}

function fitNow() {
  if (!fit || !surface.value || surface.value.clientWidth === 0) return
  fit.fit()
  if (running.value) session.resize(term.cols, term.rows).catch(() => {})
}

function scheduleFit() {
  clearTimeout(fitTimer)
  fitTimer = setTimeout(fitNow, 60)
}

// The clipboard goes through Windows itself (the core), never the browser's: a paste brings
// text, copied files as their paths, or a picture saved as a PNG whose path is pasted.
async function pasteIntoSession() {
  if (!running.value) return
  try {
    const pasted = await clipboardPaste()
    if (pasted.kind !== 'empty') term.paste(pasted.text)
  } catch {
    // Nothing pasted; the clipboard could not be read.
  }
}

function copySelection() {
  const text = term.getSelection()
  if (text) clipboardCopy(text).catch(() => {})
  term.clearSelection()
}

// Ctrl+V, Ctrl+Shift+V and Shift+Insert paste; Ctrl+C copies a selection and otherwise goes to
// the session as usual (interrupt). preventDefault stops the browser's own paste, so nothing doubles.
function clipboardKeys(event) {
  if (event.type !== 'keydown') return true
  const key = event.key.toLowerCase()
  if ((event.ctrlKey && key === 'v') || (event.shiftKey && event.key === 'Insert')) {
    event.preventDefault()
    pasteIntoSession()
    return false
  }
  if (event.ctrlKey && key === 'c' && term.hasSelection()) {
    event.preventDefault()
    copySelection()
    return false
  }
  return true
}

// Right-click, as in Windows Terminal: copy when text is selected, paste when not.
function onRightClick(event) {
  event.preventDefault()
  if (term.hasSelection()) copySelection()
  else pasteIntoSession()
}

// `greet`: true only when the member pressed Wake, so Castle opens the conversation. A wake Castle
// does itself (at launch, or on a request from elsewhere) starts a blank session as before.
async function wake({ greet = false } = {}) {
  if (running.value) {
    term.focus()
    return
  }
  problem.value = ''
  fitNow()
  // The console asks this screen where its cursor is the moment it starts, before Castle has said
  // the session is running; the answer must still reach it or the CLI never begins.
  starting = true
  try {
    show(await session.start(term.cols, term.rows, greet === true))
    ended.value = false
    term.focus()
  } catch (reason) {
    problem.value = String(reason)
  } finally {
    starting = false
  }
}

function startDrag(event) {
  const box = panel.value.parentElement.getBoundingClientRect()
  const move = (e) => {
    const percent = ((e.clientX - box.left) / box.width) * 100
    width.value = Math.round(Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, percent)))
  }
  const stop = () => {
    window.removeEventListener('pointermove', move)
    window.removeEventListener('pointerup', stop)
    try {
      localStorage.setItem(WIDTH_KEY, String(width.value))
    } catch {
      // Not kept; the width resets next time.
    }
  }
  window.addEventListener('pointermove', move)
  window.addEventListener('pointerup', stop)
  event.preventDefault()
}

onMounted(async () => {
  term = new Terminal({
    allowTransparency: true,
    allowProposedApi: true,
    cursorBlink: true,
    fontFamily: "'Cascadia Mono', Consolas, monospace",
    fontSize: 14,
    lineHeight: 1.15,
    scrollback: 10000,
    theme: {
      background: 'rgba(0, 0, 0, 0)',
      foreground: cssVar('--text'),
      cursor: cssVar('--accent'),
      selectionBackground: 'rgba(143, 184, 255, 0.35)',
    },
  })
  fit = new FitAddon()
  term.loadAddon(fit)
  term.loadAddon(new Unicode11Addon())
  term.unicode.activeVersion = '11'
  term.open(surface.value)
  term.attachCustomKeyEventHandler(clipboardKeys)
  surface.value.addEventListener('contextmenu', onRightClick)
  term.onData((data) => {
    if (running.value || starting || isTerminalReply(data)) session.write(data).catch(() => {})
  })

  // Listen before anything starts: the console asks this screen a question before it runs.
  stopListening.push(await session.onOutput((text) => term.write(text)))
  stopListening.push(
    await session.onExit(async () => {
      running.value = false
      ended.value = true
      try {
        show(await session.state())
      } catch {
        // the last status stays
      }
    }),
  )

  fitNow()
  observer = new ResizeObserver(scheduleFit)
  observer.observe(surface.value)

  // the tray's Wake and the start-up wake are the companion's main session's only
  const mainSession = !isDev && props.room !== 'story'
  if (mainSession) stopListening.push(await onWakeRequest(wake))
  try {
    stopListening.push(await getCurrentWebview().onDragDropEvent(onDragDrop))
  } catch {
    // No drop events outside Castle's window (a browser preview).
  }

  const state = await session.state()
  show(state)
  const snapshot = state.snapshot ?? (session.snapshot && state.running ? await session.snapshot().catch(() => '') : '')
  if (snapshot) term.write(snapshot)

  // Once per sign-in, and only now that this screen is listening (the main session only).
  if (mainSession && !state.running && (await takeAutoWake())) {
    if (state.status.ready) await wake()
    else problem.value = `${props.companion} could not start by itself: ${state.status.message}`
  }
})

// The memory folder may have changed in Settings, or while Castle was hidden: read it again.
async function recheck() {
  try {
    show(await session.state())
    problem.value = ''
  } catch {
    // The last status stays.
  }
}
watch(() => props.settingsStamp, recheck)
window.addEventListener('castle-provider-changed', recheck)
onBeforeUnmount(() => window.removeEventListener('castle-provider-changed', recheck))
const whenShown = () => {
  if (document.visibilityState === 'visible' && !running.value) recheck()
}
document.addEventListener('visibilitychange', whenShown)
onBeforeUnmount(() => document.removeEventListener('visibilitychange', whenShown))

// Switching CHAT and LOG, or going wide, keeps the session; the Log is refitted and, when it is
// the one showing, the cursor goes back into it.
watch([view, expanded], () => {
  scheduleFit()
  if (running.value && view.value === 'log') setTimeout(() => term.focus(), 80)
})

onBeforeUnmount(() => {
  observer?.disconnect()
  stopListening.forEach((stop) => stop())
  term?.dispose()
})

// DEV: the tab's Log button chooses CHAT or LOG; its chat is read only while the tab shows (set at
// once, before Chat's own read when it comes into view); the tab hears when the session starts or ends.
if (isDev) {
  watch(() => props.showLog, (on) => {
    if ((view.value === 'log') !== on) view.value = on ? 'log' : 'chat'
  })
  watch(() => props.shown, (on) => devChatShown(props.devSlot, on), { immediate: true, flush: 'sync' })
  watch(() => props.shown, (on) => on && scheduleFit())
  watch(running, (on) => emit('running', on))
  onBeforeUnmount(() => devChatShown(props.devSlot, false))
}

// What Chat sends goes into the same session, exactly as if typed in the Log.
function writeToSession(data) {
  if (!running.value) return Promise.resolve()
  return session.write(data)
}

// Chat's answer buttons read the menu on the Log's screen.
function screenLines(count = 25) {
  if (!term) return []
  const buffer = term.buffer.active
  const end = buffer.baseY + term.rows
  const lines = []
  for (let y = Math.max(0, end - count); y < end; y += 1) {
    lines.push(buffer.getLine(y)?.translateToString(true) ?? '')
  }
  return lines
}

// a file put in the chat from outside (the DEV tab's FILES reader), as if dropped in the box
const addPaths = (paths) => chat.value?.addPaths(paths)
defineExpose({ wake, screenLines, writeToSession, addPaths })
</script>

<template>
  <section
    ref="panel"
    class="terminal-panel"
    :class="{ wide: expanded, embedded: isDev }"
    :style="!isDev && !expanded && width ? { width: width + '%' } : null"
    :aria-label="companion"
  >
    <div v-if="!isDev" class="header">
      <div v-if="$slots.header && !expanded" class="portrait"><slot name="header" :write="writeToSession" :running="running" /></div>
      <div class="tabs" role="tablist" aria-label="Chat or log">
        <button role="tab" :aria-selected="view === 'chat'" :class="{ active: view === 'chat' }" @click="setView('chat')">CHAT</button>
        <button role="tab" :aria-selected="view === 'log'" :class="{ active: view === 'log' }" @click="setView('log')">LOG</button>
        <button v-if="hasProfile" role="tab" :aria-selected="view === 'profile'" :class="{ active: view === 'profile' }" @click="setView('profile')">PROFILE</button>
        <span class="spacer"></span>
        <button
          class="widen"
          :title="expanded ? 'Back beside the widgets' : 'Fill the screen'"
          :aria-label="expanded ? 'Back beside the widgets' : 'Fill the screen'"
          @click="expanded = !expanded"
        >{{ expanded ? '⤡' : '⤢' }}</button>
      </div>
    </div>

    <QuickActions v-if="!isDev && room !== 'story' && view !== 'profile'" :running="running" :write="writeToSession" :settings-stamp="settingsStamp" />

    <div class="body">
    <div v-show="view === 'log'" ref="surface" class="surface"></div>
    <div v-show="view === 'chat'" class="chat-area">
      <ChatView
        ref="chat"
        :running="running"
        :visible="view === 'chat' && shown"
        :write="writeToSession"
        :read-screen="screenLines"
        :companion="companion"
        :room="chatRoom"
        :cli="activeProvider || cli"
        @show-log="setView('log')"
        @busy="emit('busy', $event)"
        @context="emit('context', $event)"
      />
    </div>

    <div v-if="view === 'profile'" class="profile-area">
      <ProfileView :shown="view === 'profile'" :companion="companion" @settings="emit('settings', 'database')" />
    </div>

    <div v-if="dropping && view !== 'profile'" class="drop-hint">{{ view === 'chat' ? `Drop to share with ${companion}` : 'Drop to type the path into the Log' }}</div>

    <div v-if="!running && view !== 'profile'" class="asleep">
      <p v-if="isDev" class="title">{{ ended ? 'The session in this project ended.' : `${companion} is not working here yet.` }}</p>
      <p v-else class="title">{{ ended ? `${companion}'s session ended.` : `${companion} is resting.` }}</p>
      <p class="message">{{ problem || status.message }}</p>
      <div class="buttons">
        <button class="primary" :disabled="!status.ready" @click="wake({ greet: true })">Wake {{ companion === 'Your companion' ? 'your companion' : companion }}{{ isDev ? ' here' : '' }}</button>
        <button v-if="status.fix === 'setup'" @click="emit('setup')">Open setup</button>
        <button v-if="status.fix === 'settings' || status.fix === 'install'" @click="emit('settings')">Settings</button>
      </div>
    </div>
    </div>

    <div v-if="!expanded && !isDev" class="handle" title="Drag to widen" @pointerdown="startDrag"></div>
  </section>
</template>

<style scoped>
.terminal-panel {
  position: absolute;
  top: var(--panel-top, 24px);
  bottom: var(--panel-bottom, 24px);
  left: var(--panel-left, 24px);
  right: var(--panel-right, auto);
  width: var(--panel-width, 40%);
  height: var(--panel-height, auto);
  display: flex;
  flex-direction: column;
  gap: 12px;
  padding: 14px 6px 14px 16px;
  background: var(--panel-background);
  border: 1px solid var(--line);
  border-radius: 14px;
  backdrop-filter: blur(var(--glass-blur));
  box-sizing: border-box;
}

.terminal-panel.wide {
  top: 16px;
  bottom: 16px;
  left: 16px;
  right: auto;
  width: calc(100% - 32px);
  height: auto;
}

/* A DEV tab's session: it fills its place in the tab instead of floating over HOME. */
.terminal-panel.embedded {
  position: relative;
  inset: auto;
  flex: 1;
  width: auto;
  height: auto;
  min-width: 0;
  min-height: 0;
  padding: 12px 6px 12px 14px;
}

.header {
  display: flex;
  flex-direction: column;
  gap: 10px;
  flex: none;
}

.tabs {
  display: flex;
  align-items: center;
  gap: 4px;
  padding-right: 10px;
  border-bottom: 1px solid var(--line);
}

.tabs button {
  padding: 6px 14px;
  border: 0;
  border-bottom: 2px solid transparent;
  border-radius: 0;
  background: none;
  color: var(--text-dim);
  font-size: 12px;
  letter-spacing: 0.14em;
}

.tabs button.active {
  color: var(--text);
  border-bottom-color: var(--accent);
}

.tabs .spacer {
  flex: 1;
}

.tabs .widen {
  padding: 4px 8px;
  font-size: 16px;
  letter-spacing: 0;
}

.chat-area {
  display: flex;
  flex: 1;
  min-width: 0;
  min-height: 0;
}

.profile-area {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
}

.chat-soon {
  margin: auto;
  font-size: 13px;
  color: var(--text-dim);
}

.body {
  position: relative;
  display: flex;
  flex: 1;
  min-height: 0;
}

.surface {
  flex: 1;
  min-width: 0;
  min-height: 0;
}

.surface :deep(.xterm-viewport) {
  background-color: transparent !important;
  scrollbar-width: thin;
  scrollbar-color: var(--line) transparent;
}

.surface :deep(.xterm-viewport::-webkit-scrollbar) {
  width: 8px;
}

.surface :deep(.xterm-viewport::-webkit-scrollbar-track) {
  background: transparent;
}

.surface :deep(.xterm-viewport::-webkit-scrollbar-thumb) {
  background: var(--line);
  border-radius: 4px;
}

.drop-hint {
  position: absolute;
  inset: 6px 16px 6px 6px;
  z-index: 6;
  display: grid;
  place-items: center;
  border: 2px dashed var(--accent);
  border-radius: 14px;
  background: color-mix(in srgb, var(--accent) 14%, rgba(4, 10, 30, 0.6));
  color: var(--text);
  font-size: 15px;
  pointer-events: none;
}

.asleep {
  position: absolute;
  inset: 0 10px 0 0;
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 8px;
  padding: 24px;
  text-align: center;
  border-radius: 14px;
  background: var(--panel-background);
}

.title {
  margin: 0;
  font-size: 18px;
  font-weight: 600;
}

.message {
  margin: 0 0 8px;
  max-width: 420px;
  font-size: 13px;
  color: var(--text-dim);
}

.buttons {
  display: flex;
  gap: 8px;
}

button {
  padding: 8px 16px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 14px;
  cursor: pointer;
}

button.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: #07142e;
  font-weight: 600;
}

button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

button:hover:not(:disabled) {
  filter: brightness(1.12);
}

button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

.handle {
  position: absolute;
  top: 12px;
  right: -5px;
  bottom: 12px;
  width: 10px;
  cursor: ew-resize;
}

.handle:hover {
  background: linear-gradient(to right, transparent 4px, var(--accent) 4px, var(--accent) 6px, transparent 6px);
}
</style>
