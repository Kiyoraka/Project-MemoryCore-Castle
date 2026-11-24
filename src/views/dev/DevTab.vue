<script setup>
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import TerminalPanel from '../../components/TerminalPanel.vue'
import ContextMeter from '../../components/ContextMeter.vue'
import { typeLine } from '../../components/typeline.js'
import PlanPanel from './PlanPanel.vue'
import SourcePanel from './SourcePanel.vue'
import ProjectDatabase from './ProjectDatabase.vue'
import PhonePanel from './PhonePanel.vue'
import { useSaveClose } from './useSaveClose.js'
import { devCloseTab, memoryFeatures, mobileCheck } from '../../api/castle.js'
import { devOpenLink } from '../../api/devgit.js'
import { linkOf, programOf } from './devprojects.js'
import { tabLabel } from './devtabs.js'

// One project's session in the DEV room: its chat in the middle, the Log behind a button, the
// plan beside it on request. It stays mounted while other tabs show, so the session keeps going.
const props = defineProps({
  // { slot, folder, name, running, status: { ready, message, fix } }
  tab: { type: Object, required: true },
  // this tab is the one showing (and the DEV room is open)
  shown: { type: Boolean, default: false },
  companion: { type: String, default: 'Your companion' },
  cli: { type: String, default: 'claude' },
  settingsStamp: { type: Number, default: 0 },
  // this tab's live servers (started in its session, or running in its folder)
  servers: { type: Array, default: () => [] },
  // a new project's setup sentence, typed once after the companion's first reply ends ('' = none)
  firstWords: { type: String, default: '' },
})
// `state`: a fresh dev state (after Wake or Close session); `running`: the session started or ended
const emit = defineEmits(['state', 'running', 'settings', 'setup', 'first-said'])

const panel = ref(null)
const logOpen = ref(false)
const planOpen = ref(false)
const sourceOpen = ref(false)
// the project's own database (Local and Production), wide beside the chat
const databaseOpen = ref(false)
// a Flutter or Android project gets a Phone (Mobile setup, then the docked phone); read once when shown
const phoneOpen = ref(false)
const mobileKind = ref(null)
let mobileRead = false
watch(
  () => props.shown,
  async (now) => {
    if (!now || mobileRead) return
    mobileRead = true
    try {
      mobileKind.value = (await mobileCheck(props.tab.slot))?.kind ?? null
    } catch {
      mobileKind.value = null
    }
  },
  { immediate: true },
)
// what the chat reads off the session: working or not, and how full her context is
const busy = ref(false)
const context = ref(null)
// the member's MemoryCore keeps projects, so `save project` means something to her
const canSave = ref(false)
onMounted(async () => {
  try {
    canSave.value = !!(await memoryFeatures())?.projects
  } catch {
    canSave.value = false
  }
})
// what the session panel heard last (it hears before the room's next read of the tabs)
const liveRunning = ref(null)
const running = computed(() => liveRunning.value ?? !!props.tab.running)
watch(() => props.tab.running, () => (liveRunning.value = null))

// Close session: asked first, in the page, when the project has work not yet committed
const askingClose = ref(false)
// "shop · 2": the project and this session's number there
const label = computed(() => tabLabel(props.tab))
const keepButton = ref(null)
const closing = ref(false)
const problem = ref('')

function onRunning(on) {
  liveRunning.value = on
  emit('running', on)
  if (!on) {
    saveClose.sessionEnded()
    context.value = null
  }
}

// Types a line into her session (text, then Enter after a pause, as the quick actions do).
function say(text) {
  return typeLine((data) => panel.value?.writeToSession(data), text)
}

// A new project's companion hears its setup once: after it has woken and its first reply ended.
let sawWork = false
watch(busy, (now, before) => {
  if (now) sawWork = true
  if (props.firstWords && running.value && sawWork && before && !now) {
    const words = props.firstWords
    emit('first-said')
    say(words)
  }
})

function saveProject() {
  problem.value = ''
  say('save project')
}

// Close session saves first when the project has uncommitted work (or asks, without projects)
const saveClose = useSaveClose({
  slot: () => props.tab.slot,
  say,
  busy,
  running,
  canSave: () => canSave.value,
  close: () => closeNow(),
})
watch(
  () => saveClose.saving.value?.ask,
  async (ask) => {
    askingClose.value = !!ask
    if (ask) {
      // the safe answer has the focus, so Enter keeps it open and Esc does too
      await nextTick()
      keepButton.value?.focus()
    }
  },
)

// a file from the FILES reader, put in her message box
function putInChat(path) {
  panel.value?.addPaths([path])
}
// ✕ on the tab strip: an idle session ends at once; while the companion is working it asks first.
// The project stays on the list either way (the header's Close session keeps its save-first way).
const askingEnd = ref(false)
const keepRunning = ref(null)
async function endFromStrip() {
  problem.value = ''
  if (!busy.value) return closeNow()
  askingEnd.value = true
  await nextTick()
  keepRunning.value?.focus()
}
function endNow() {
  askingEnd.value = false
  closeNow()
}
defineExpose({ putInChat, endFromStrip })

function wake() {
  problem.value = ''
  panel.value?.wake({ greet: true })
}

async function askClose() {
  problem.value = ''
  await saveClose.requestClose()
}

async function closeNow() {
  askingClose.value = false
  saveClose.dismiss()
  closing.value = true
  try {
    emit('state', await devCloseTab(props.tab.slot))
  } catch (reason) {
    problem.value = String(reason)
  } finally {
    closing.value = false
  }
}
</script>

<template>
  <section class="dev-tab" :aria-label="label">
    <header class="tab-head">
      <span class="dot" :class="{ on: running }" :title="running ? 'Working' : 'Asleep'"></span>
      <div class="names">
        <h2 :title="label">{{ label }}</h2>
        <span class="folder" :title="tab.folder">{{ tab.folder }}</span>
      </div>
      <div class="actions">
        <ContextMeter :context="context" />
        <button v-if="!running" class="primary" :disabled="!tab.status?.ready" :title="tab.status?.ready ? '' : tab.status?.message" @click="wake">Wake</button>
        <button v-if="running && canSave" :disabled="!!saveClose.saving.value" title="Ask her to save the project: its record in your memory, and a commit" @click="saveProject">Save project</button>
        <button :class="{ on: sourceOpen }" :aria-pressed="sourceOpen" @click="sourceOpen = !sourceOpen">Source</button>
        <button :class="{ on: planOpen }" :aria-pressed="planOpen" @click="planOpen = !planOpen">Plan</button>
        <button :class="{ on: databaseOpen }" :aria-pressed="databaseOpen" title="This project's own database: Local and Production" @click="databaseOpen = !databaseOpen">Database</button>
        <button v-if="mobileKind" :class="{ on: phoneOpen }" :aria-pressed="phoneOpen" title="A phone for this app: Mobile setup, then the phone itself" @click="phoneOpen = !phoneOpen">Phone</button>
        <button :class="{ on: logOpen }" :aria-pressed="logOpen" @click="logOpen = !logOpen">Log</button>
        <button :disabled="closing" @click="askClose">{{ closing ? 'Closing…' : 'Close session' }}</button>
      </div>
    </header>
    <p v-if="servers.length" class="live">
      <span class="label">LIVE</span>
      <button v-for="s in servers" :key="s.port" class="server" :title="`${programOf(s)} - pid ${s.pid}`" @click="devOpenLink(linkOf(s)).catch(() => {})">
        :{{ s.port }} {{ programOf(s) }} <span class="go">open &#8599;</span>
      </button>
    </p>
    <p v-if="problem" class="problem" role="alert">{{ problem }}</p>
    <div v-if="saveClose.saving.value && !saveClose.saving.value.ask" class="saving" role="status">
      <span>{{ saveClose.saving.value.note }}</span>
      <button @click="saveClose.closeAnyway()">Close without saving</button>
      <button class="primary" @click="saveClose.dismiss()">Keep it open</button>
    </div>

    <div class="tab-body">
      <TerminalPanel
        ref="panel"
        :dev-slot="tab.slot"
        :shown="shown"
        :show-log="logOpen"
        :companion="companion"
        :cli="cli"
        :settings-stamp="settingsStamp"
        @view="logOpen = $event === 'log'"
        @busy="busy = $event"
        @context="context = $event"
        @running="onRunning"
        @state="emit('state', $event)"
        @settings="emit('settings', $event)"
        @setup="emit('setup')"
      />
      <SourcePanel v-if="sourceOpen" class="plan-side" :slot="tab.slot" :folder="tab.folder" :companion="companion" :shown="shown && sourceOpen" @ask="say" />
      <PlanPanel v-if="planOpen" class="plan-side" :index="tab.slot" :shown="shown && planOpen" />
      <ProjectDatabase v-if="databaseOpen" :slot="tab.slot" :shown="shown && databaseOpen" />
      <PhonePanel v-if="phoneOpen" :index="tab.slot" :shown="shown && phoneOpen" :running="running" :say="say" :busy="busy" />
    </div>

    <div v-if="askingEnd" class="close-sheet" role="dialog" aria-modal="true" aria-labelledby="end-sheet-title" @click.self="askingEnd = false" @keydown.esc.stop="askingEnd = false">
      <div class="close-card">
        <h3 id="end-sheet-title">End {{ label }}?</h3>
        <p>{{ companion }} is still working in this session. Ending it stops that work. {{ tab.name }} stays in your projects.</p>
        <div class="close-buttons">
          <button ref="keepRunning" class="primary" @click="askingEnd = false">Keep it running</button>
          <button @click="endNow">End session</button>
        </div>
      </div>
    </div>

    <div v-if="askingClose" class="close-sheet" role="dialog" aria-modal="true" aria-labelledby="close-sheet-title" @click.self="askingClose = false; saveClose.dismiss()" @keydown.esc.stop="askingClose = false; saveClose.dismiss()">
      <div class="close-card">
        <h3 id="close-sheet-title">Close this session?</h3>
        <p>{{ tab.name }} has changes that are not committed yet. Closing the session keeps the files as they are, but {{ companion }} stops working here.</p>
        <div class="close-buttons">
          <button ref="keepButton" class="primary" @click="askingClose = false; saveClose.dismiss()">Keep it open</button>
          <button @click="closeNow">Close anyway</button>
        </div>
      </div>
    </div>
  </section>
</template>

<style scoped>
.dev-tab {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 10px;
  flex: 1;
  min-width: 0;
  min-height: 0;
}

.saving {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  padding: 8px 14px;
  font-size: 13px;
  background: var(--panel-background);
  border: 1px solid var(--line);
  border-radius: 12px;
}
.saving span {
  flex: 1;
}
.tab-head {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 10px 12px;
  padding: 10px 14px;
  background: var(--panel-background);
  border: 1px solid var(--line);
  border-radius: 14px;
  -webkit-backdrop-filter: blur(var(--glass-blur)); backdrop-filter: blur(var(--glass-blur));
}

.dot {
  flex: none;
  width: 9px;
  height: 9px;
  border-radius: 50%;
  background: var(--text-dim);
  opacity: 0.5;
}

.dot.on {
  background: var(--good);
  opacity: 1;
  box-shadow: 0 0 8px var(--good);
}

.names {
  display: flex;
  flex-direction: column;
  flex: 1 1 160px;
  min-width: 0;
}

h2 {
  margin: 0;
  overflow: hidden;
  font-size: 16px;
  font-weight: 600;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.folder {
  overflow: hidden;
  font-size: 11px;
  color: var(--text-dim);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.actions {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
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

button.on {
  border-color: var(--accent);
  background: var(--hover);
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

.live {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 6px;
  margin: 0;
  font-size: 12px;
}

.live .label {
  letter-spacing: 0.14em;
  color: var(--good, #8fe3b0);
}

.server {
  padding: 3px 9px;
  border: 1px solid color-mix(in srgb, var(--good, #8fe3b0) 45%, transparent);
  border-radius: 999px;
  background: color-mix(in srgb, var(--good, #8fe3b0) 8%, transparent);
  color: inherit;
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}

.server .go {
  color: var(--good, #8fe3b0);
}

.problem {
  margin: 0;
  padding: 0 4px;
  font-size: 13px;
  color: var(--warn);
}

.tab-body {
  display: flex;
  gap: 12px;
  flex: 1;
  min-width: 0;
  min-height: 0;
}

.plan-side {
  flex: 0 1 320px;
  min-width: 0;
}

.close-sheet {
  position: absolute;
  inset: 0;
  z-index: 20;
  display: grid;
  place-items: center;
  padding: 16px;
  border-radius: 14px;
  background: rgba(6, 4, 20, 0.42);
  -webkit-backdrop-filter: blur(3px); backdrop-filter: blur(3px);
}

.close-card {
  width: min(420px, 100%);
  padding: 20px 22px;
  border: 1px solid var(--line);
  border-radius: 14px;
  background: var(--panel-background);
  -webkit-backdrop-filter: blur(var(--glass-blur)); backdrop-filter: blur(var(--glass-blur));
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.45);
  box-sizing: border-box;
}

.close-card h3 {
  margin: 0 0 8px;
  font-size: 16px;
  font-weight: 600;
}

.close-card p {
  margin: 0 0 16px;
  font-size: 13px;
  line-height: 1.5;
  color: var(--text-dim);
  overflow-wrap: anywhere;
}

.close-buttons {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 8px;
}
</style>
