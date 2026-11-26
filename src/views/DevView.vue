<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import DevTab from './dev/DevTab.vue'
import FileTree from './dev/FileTree.vue'
import FileView from './dev/FileView.vue'
import NewProject from './dev/NewProject.vue'
import LiveServers from './dev/LiveServers.vue'
import { serversOf } from './dev/devprojects.js'
import { chooseFolder, devNewSession, devOpen, devRemoveProject, devServers, devState } from '../api/castle.js'
import { MAX_TABS, folderName, newSlot, nextActive, orderTabs, recentProjects, removeQuestion, samePath, tabForFolder, tabLabel } from './dev/devtabs.js'

// The DEV room: the projects on the left, kept until the member removes one; the sessions along
// the top, up to eight in all, several per project if wanted ("shop · 1", "shop · 2"). A tab keeps
// working while another shows. Ending a session keeps its project; removing a project asks first,
// names the sessions that will end, and ends them with it.
const props = defineProps({
  // the DEV room is the one showing
  visible: { type: Boolean, default: false },
  companion: { type: String, default: 'Your companion' },
  cli: { type: String, default: 'claude' },
  settingsStamp: { type: Number, default: 0 },
})
const emit = defineEmits(['settings', 'setup'])

// the tabs' running dots are read again this often while the room shows
const READ_EVERY_MS = 3000

const state = ref({ tabs: [], projects: [], memoryFolder: '' })
const activeSlot = ref(null)
const problem = ref('')
const busy = ref(false)
let timer

const tabs = computed(() => orderTabs(state.value.tabs).map((t) => ({ ...t, name: t.name || folderName(t.folder), label: tabLabel(t) })))
const projects = computed(() => recentProjects(state.value.projects, state.value.tabs, state.value.memoryFolder))
const full = computed(() => tabs.value.length >= MAX_TABS)

// the left column: the recent PROJECTS, or the open project's FILES
const side = ref('projects')
// the file open in the reader ({ name, rel }), for the active tab
const reading = ref(null)
const tabViews = {}
const activeTab = computed(() => tabs.value.find((t) => t.slot === activeSlot.value) ?? null)
watch(activeSlot, () => (reading.value = null))
watch(activeTab, (tab) => {
  if (!tab && side.value === 'files') side.value = 'projects'
})
// Put in chat: the file goes into the active tab's message box, as a dropped file does
function putInChat(path) {
  reading.value = null
  tabViews[activeSlot.value]?.putInChat(path)
}

function apply(next, wantedFolder = null, wantedSlot = null) {
  if (!next) return
  state.value = { tabs: next.tabs ?? [], projects: next.projects ?? [], memoryFolder: next.memoryFolder ?? '' }
  const shown = wantedSlot ?? nextActive(state.value.tabs, activeSlot.value, wantedFolder)
  activeSlot.value = state.value.tabs.some((t) => t.slot === shown) ? shown : nextActive(state.value.tabs, activeSlot.value)
}

// A project chosen on the list with no session running shows its own card ("Start a session")
// instead of a tab; it never opens a terminal by itself.
const idleFolder = ref(null)
const idleProject = computed(() => projects.value.find((p) => samePath(p.folder, idleFolder.value)) ?? null)
watch(activeSlot, (slot, before) => {
  if (slot !== before && slot !== null) idleFolder.value = null
})
watch(projects, (list) => {
  const p = list.find((x) => samePath(x.folder, idleFolder.value))
  if (!p || p.count) idleFolder.value = null
})
function isSelected(project) {
  if (idleFolder.value) return samePath(idleFolder.value, project.folder)
  return !!activeTab.value && samePath(activeTab.value.folder, project.folder)
}
function selectProject(project) {
  problem.value = ''
  menuFor.value = null
  if (project.count) {
    idleFolder.value = null
    activeSlot.value = project.slot
  } else {
    idleFolder.value = project.folder
  }
}
function showTab(slot) {
  idleFolder.value = null
  activeSlot.value = slot
}

// + on a project: one more session there, in the first free tab
async function startSession(folder) {
  problem.value = ''
  menuFor.value = null
  busy.value = true
  try {
    const before = state.value.tabs
    const next = await devNewSession(folder)
    idleFolder.value = null
    apply(next, null, newSlot(before, next?.tabs))
  } catch (reason) {
    problem.value = String(reason)
  } finally {
    busy.value = false
  }
}

// ✕ on a tab: the session ends (asking first only while the companion is working); the project stays
function endSession(slot) {
  problem.value = ''
  tabViews[slot]?.endFromStrip()
}

// the ⋯ menu of one project, and Remove project… with its question
const menuFor = ref(null)
const removing = ref(null)
const removeAsk = computed(() => (removing.value ? removeQuestion(removing.value, state.value.tabs) : null))
const keepButton = ref(null)
function isMenu(project) {
  return samePath(menuFor.value, project.folder)
}
function toggleMenu(folder) {
  menuFor.value = samePath(menuFor.value, folder) ? null : folder
}
async function askRemove(project) {
  menuFor.value = null
  removing.value = project
  await nextTick()
  keepButton.value?.focus()
}
async function confirmRemove() {
  const project = removing.value
  removing.value = null
  if (!project) return
  problem.value = ''
  try {
    apply(await devRemoveProject(project.folder))
    if (samePath(idleFolder.value, project.folder)) idleFolder.value = null
  } catch (reason) {
    problem.value = String(reason)
  }
}
function closeMenus(event) {
  if (!event.target.closest?.('.row-menu, .row-more')) menuFor.value = null
}
onMounted(() => document.addEventListener('click', closeMenus))
onBeforeUnmount(() => document.removeEventListener('click', closeMenus))

async function refresh() {
  if (document.visibilityState !== 'visible') return
  try {
    apply(await devState())
  } catch {
    // outside Castle's window, or the core is busy: the last list stays
  }
}

async function openProject(folder) {
  problem.value = ''
  idleFolder.value = null
  const open = tabForFolder(state.value.tabs, folder)
  if (open) {
    activeSlot.value = open.slot
    return
  }
  busy.value = true
  try {
    apply(await devOpen(folder), folder)
  } catch (reason) {
    problem.value = String(reason)
  } finally {
    busy.value = false
  }
}

async function openFolder() {
  problem.value = ''
  let folder
  try {
    folder = await chooseFolder('Open a project folder')
  } catch (reason) {
    problem.value = String(reason)
    return
  }
  if (folder) await openProject(folder)
}

// New project: the sheet clones or creates the repo, then the new folder opens as a tab
const making = ref(false)
// The setup sentence a new project's companion hears once, by folder, until its tab says it.
const firstWords = ref({})
async function made(folder, sentence = '') {
  making.value = false
  if (!folder) return
  if (sentence) firstWords.value = { ...firstWords.value, [folder]: sentence }
  await openProject(folder)
}
function firstSaid(folder) {
  const { [folder]: _, ...rest } = firstWords.value
  firstWords.value = rest
}
// a tab closed before its companion heard its setup: the sentence goes with it
watch(
  () => state.value.tabs.map((t) => t.folder),
  (open) => {
    for (const folder of Object.keys(firstWords.value)) if (!open.includes(folder)) firstSaid(folder)
  },
)

// a tab's session started or ended: its dot changes now, before the next read
function markRunning(slot, on) {
  state.value = { ...state.value, tabs: state.value.tabs.map((t) => (t.slot === slot ? { ...t, running: on } : t)) }
}

// The live servers on this computer: read every few seconds while DEV is in sight. View only.
const servers = ref([])
const serversOpen = ref(false)
const SERVERS_EVERY_MS = 5000
let serverTimer
async function readServers() {
  try {
    servers.value = await devServers()
  } catch {
    // the next look tries again
  }
}

watch(
  () => props.visible,
  (on) => {
    clearInterval(timer)
    clearInterval(serverTimer)
    if (!on) return
    refresh()
    readServers()
    timer = setInterval(refresh, READ_EVERY_MS)
    serverTimer = setInterval(readServers, SERVERS_EVERY_MS)
  },
)
onMounted(() => {
  refresh()
  if (props.visible) {
    readServers()
    timer = setInterval(refresh, READ_EVERY_MS)
    serverTimer = setInterval(readServers, SERVERS_EVERY_MS)
  }
})
onBeforeUnmount(() => {
  clearInterval(timer)
  clearInterval(serverTimer)
})
</script>

<template>
  <div class="dev-room">
    <aside class="projects widget" :aria-label="side === 'files' ? 'Files' : 'Projects'">
      <div class="side-switch" role="tablist" aria-label="Left column">
        <button role="tab" :aria-selected="side === 'projects'" :class="{ on: side === 'projects' }" @click="side = 'projects'">PROJECTS</button>
        <button role="tab" :aria-selected="side === 'files'" :class="{ on: side === 'files' }" :disabled="!activeTab" :title="activeTab ? `The files of ${activeTab.name}` : 'Open a project to see its files'" @click="side = 'files'">FILES</button>
      </div>
      <FileTree
        v-if="activeTab"
        v-show="side === 'files'"
        :key="`files:${activeTab.slot}:${activeTab.folder}`"
        :slot="activeTab.slot"
        :folder="activeTab.folder"
        :shown="visible && side === 'files'"
        :selected="reading?.rel"
        @open="reading = $event"
      />
      <template v-if="side === 'projects'">
      <button class="open-folder new-project" :disabled="busy" title="Clone a repo from a link, or create a new one on GitHub" @click="making = true">+ New project</button>
      <button class="open-folder" :disabled="busy" :title="full ? `All ${MAX_TABS} tabs are in use - close a session to open another project` : ''" @click="openFolder">
        {{ busy ? 'Opening…' : 'Open a folder…' }}
      </button>
      <p v-if="problem" class="problem" role="alert">{{ problem }}</p>
      <p v-if="!projects.length" class="empty">The project folders you open are kept here.</p>
      <ul class="recent">
        <li v-for="project in projects" :key="project.folder" :class="{ active: isSelected(project), missing: project.missing }">
          <button class="project" :title="project.folder" :disabled="busy" @click="selectProject(project)">
            <span class="name">
              <span class="dot" :class="{ on: project.slots.some((slot) => tabs.find((t) => t.slot === slot)?.running) }"></span>
              <span class="pname">{{ project.name }}</span>
              <span class="sessions" :class="{ live: project.count }" :aria-label="`${project.count} ${project.count === 1 ? 'session' : 'sessions'} open`">{{ project.count }}</span>
            </span>
            <span class="folder">{{ project.missing ? 'Folder not found' : project.folder }}</span>
          </button>
          <button
            class="row-btn"
            :disabled="busy || full || project.missing"
            :title="project.missing ? 'The folder is not on this computer' : full ? `All ${MAX_TABS} sessions are in use - end one first` : `Start a session in ${project.name}`"
            :aria-label="`Start a session in ${project.name}`"
            @click="startSession(project.folder)"
          >+</button>
          <button class="row-btn row-more" aria-haspopup="menu" :aria-expanded="isMenu(project)" :aria-label="`More for ${project.name}`" @click="toggleMenu(project.folder)">⋯</button>
          <div v-if="isMenu(project)" class="row-menu" role="menu" @keydown.esc.stop="menuFor = null">
            <button role="menuitem" :disabled="busy || full || project.missing" @click="startSession(project.folder)">New session</button>
            <button role="menuitem" class="danger" @click="askRemove(project)">Remove project…</button>
          </div>
        </li>
      </ul>
      </template>
    </aside>

    <div class="work">
      <nav class="tab-strip" role="tablist" aria-label="Open sessions">
        <div v-for="tab in tabs" :key="tab.slot" class="strip-tab" :class="{ active: tab.slot === activeSlot && !idleProject }">
          <button
            role="tab"
            class="strip-open"
            :aria-selected="tab.slot === activeSlot && !idleProject"
            :title="tab.folder"
            @click="showTab(tab.slot)"
          >
            <span class="dot" :class="{ on: tab.running }"></span>
            <span class="label">{{ tab.label }}</span>
          </button>
          <button class="strip-close" :aria-label="`End ${tab.label}`" :title="`End ${tab.label}. ${tab.name} stays in your projects.`" @click="endSession(tab.slot)">✕</button>
        </div>
        <span v-if="tabs.length" class="count">{{ tabs.length }} / {{ MAX_TABS }}</span>
        <button class="servers-btn" :class="{ alone: !tabs.length }" :aria-expanded="serversOpen" title="The developer servers listening on this computer, and which project owns each" @click="serversOpen = !serversOpen">
          SERVERS: {{ servers.length }} live {{ serversOpen ? '▴' : '▾' }}
        </button>
      </nav>

      <DevTab
        v-for="tab in tabs"
        v-show="tab.slot === activeSlot && !idleProject"
        :key="`${tab.slot}:${tab.folder}`"
        :ref="(view) => (view ? (tabViews[tab.slot] = view) : delete tabViews[tab.slot])"
        :tab="tab"
        :servers="serversOf(servers, tab.slot)"
        :shown="visible && tab.slot === activeSlot"
        :companion="companion"
        :cli="cli"
        :settings-stamp="settingsStamp"
        :first-words="firstWords[tab.folder] ?? ''"
        @first-said="firstSaid(tab.folder)"
        @state="apply($event)"
        @running="markRunning(tab.slot, $event)"
        @settings="emit('settings', $event)"
        @setup="emit('setup')"
      />

      <div v-if="idleProject" class="nothing widget">
        <p class="title">{{ idleProject.name }}</p>
        <template v-if="idleProject.missing">
          <p>This folder is no longer on this computer ({{ idleProject.folder }}). Put it back, or take the project off the list.</p>
          <button class="open-folder" @click="askRemove(idleProject)">Remove project…</button>
        </template>
        <template v-else>
          <p>No session is running here. Start one and {{ companion }} works in {{ idleProject.name }} in a session of its own.</p>
          <button class="open-folder start" :disabled="busy || full" :title="full ? `All ${MAX_TABS} sessions are in use - end one first` : ''" @click="startSession(idleProject.folder)">Start a session</button>
        </template>
      </div>

      <FileView v-if="reading && activeTab && !idleProject" :slot="activeTab.slot" :folder="activeTab.folder" :entry="reading" @put="putInChat" @close="reading = null" />

      <div v-if="!tabs.length && !idleProject" class="nothing widget">
        <p class="title">No project is open.</p>
        <p>Choose one on the left, open a folder, or start a new project, and {{ companion }} works there in a session of its own.</p>
      </div>
    </div>

    <LiveServers v-if="serversOpen" :servers="servers" :tabs="tabs" @close="serversOpen = false" />
    <div v-if="removeAsk" class="remove-sheet" role="dialog" aria-modal="true" aria-labelledby="remove-title" @click.self="removing = null" @keydown.esc.stop="removing = null">
      <div class="remove-card">
        <h3 id="remove-title">{{ removeAsk.title }}</h3>
        <p>{{ removeAsk.text }}</p>
        <div class="remove-buttons">
          <button ref="keepButton" class="primary" @click="removing = null">Keep it</button>
          <button class="danger" @click="confirmRemove">{{ removeAsk.button }}</button>
        </div>
      </div>
    </div>
    <NewProject v-if="making" :companion="companion" @made="made" @close="making = false" />
  </div>
</template>

<style scoped>
.dev-room {
  position: absolute;
  inset: 24px;
  display: grid;
  grid-template-columns: minmax(180px, 260px) minmax(0, 1fr);
  gap: 18px;
  min-width: 0;
}

.widget {
  background: var(--panel-background);
  border: 1px solid var(--line);
  border-radius: 14px;
  -webkit-backdrop-filter: blur(var(--glass-blur)); backdrop-filter: blur(var(--glass-blur));
}

.side-switch {
  display: flex;
  gap: 6px;
  margin-bottom: 4px;
}
/* PROJECTS / FILES: tabs in the theme's colours, the chosen one in the accent */
.side-switch button {
  flex: 1;
  padding: 7px 8px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: none;
  color: var(--text-dim);
  font-size: 12px;
  letter-spacing: 0.12em;
}
.side-switch button:hover:not(:disabled) {
  background: var(--hover);
  color: var(--text);
}
.side-switch button.on {
  border-color: var(--accent);
  background: var(--hover);
  color: var(--accent);
}
.projects {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-width: 0;
  min-height: 0;
  padding: 18px 14px;
  overflow-y: auto;
}

h2 {
  margin: 0 0 4px;
  font-size: 13px;
  font-weight: 500;
  letter-spacing: 0.2em;
  color: var(--accent);
}

button {
  font: inherit;
  cursor: pointer;
}

button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

button:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.open-folder {
  padding: 7px 12px;
  border: 1px solid var(--accent);
  border-radius: 8px;
  background: var(--hover);
  color: var(--text);
  font-size: 13px;
  text-align: left;
}

.open-folder:hover:not(:disabled) {
  filter: brightness(1.12);
}

.problem {
  margin: 0;
  font-size: 13px;
  color: var(--warn);
  overflow-wrap: anywhere;
}

.empty {
  margin: 4px 0;
  font-size: 13px;
  color: var(--text-dim);
}

.recent {
  display: flex;
  flex-direction: column;
  gap: 2px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.recent li {
  display: flex;
  align-items: stretch;
  min-width: 0;
  border: 1px solid transparent;
  border-radius: 8px;
}

.recent li:hover {
  background: var(--hover);
}

.recent li.active {
  border-color: var(--accent);
  background: var(--hover);
}

.project {
  display: flex;
  flex-direction: column;
  flex: 1;
  min-width: 0;
  padding: 6px 4px 6px 10px;
  border: 0;
  background: none;
  color: var(--text);
  text-align: left;
}

.project .name {
  display: flex;
  align-items: center;
  gap: 7px;
  overflow: hidden;
  font-size: 14px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.project .folder {
  overflow: hidden;
  font-size: 11px;
  color: var(--text-dim);
  text-overflow: ellipsis;
  white-space: nowrap;
}

.recent li {
  position: relative;
}

.recent li.missing .folder {
  color: var(--warn);
}

.project .pname {
  overflow: hidden;
  text-overflow: ellipsis;
}

/* how many sessions the project has open: green while any is open */
.sessions {
  flex: none;
  min-width: 18px;
  padding: 0 5px;
  border: 1px solid var(--line);
  border-radius: 999px;
  color: var(--text-dim);
  font-size: 11px;
  line-height: 16px;
  text-align: center;
  font-variant-numeric: tabular-nums;
}

.sessions.live {
  border-color: var(--good);
  color: var(--good);
}

.row-btn {
  flex: none;
  width: 26px;
  border: 0;
  border-radius: 6px;
  background: none;
  color: var(--text-dim);
  font-size: 16px;
  opacity: 0.7;
  cursor: pointer;
}

.row-btn:hover:not(:disabled),
.row-btn:focus-visible,
.row-btn[aria-expanded='true'] {
  color: var(--text);
  opacity: 1;
}

.row-btn:disabled {
  opacity: 0.3;
  cursor: not-allowed;
}

.row-menu {
  position: absolute;
  top: calc(100% - 4px);
  right: 4px;
  z-index: 10;
  display: flex;
  flex-direction: column;
  min-width: 170px;
  padding: 4px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--panel-background);
  -webkit-backdrop-filter: blur(var(--glass-blur)); backdrop-filter: blur(var(--glass-blur));
  box-shadow: 0 12px 30px rgba(0, 0, 0, 0.35);
}

.row-menu button {
  padding: 7px 10px;
  border: 0;
  border-radius: 7px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 13px;
  text-align: left;
  cursor: pointer;
}

.row-menu button:hover:not(:disabled) {
  background: var(--hover);
}

.row-menu button:disabled {
  color: var(--text-dim);
  cursor: not-allowed;
}

.row-menu .danger,
.remove-buttons .danger {
  color: var(--warn);
}

.dot {
  flex: none;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--text-dim);
  opacity: 0.5;
}

.dot.on {
  background: var(--good);
  opacity: 1;
  box-shadow: 0 0 6px var(--good);
}

.work {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-width: 0;
  min-height: 0;
}

.tab-strip {
  display: flex;
  flex: none;
  align-items: center;
  gap: 4px;
  min-width: 0;
  overflow-x: auto;
  border-bottom: 1px solid var(--line);
  scrollbar-width: var(--scrollbar-width);
  scrollbar-color: var(--scrollbar-thumb) var(--scrollbar-track);
}

.strip-tab {
  display: flex;
  flex: 0 1 auto;
  align-items: center;
  gap: 2px;
  min-width: 90px;
  max-width: 220px;
  padding: 0 4px 0 0;
  border: 0;
  border-bottom: 2px solid transparent;
  border-radius: 8px 8px 0 0;
  background: none;
  color: var(--text-dim);
  font-size: 13px;
}

.strip-tab:hover {
  background: var(--hover);
}

.strip-tab.active {
  color: var(--text);
  border-bottom-color: var(--accent);
  background: var(--panel-background);
}

.strip-open {
  display: flex;
  flex: 1;
  align-items: center;
  gap: 7px;
  min-width: 0;
  padding: 7px 4px 7px 12px;
  border: 0;
  background: none;
  color: inherit;
  font: inherit;
  cursor: pointer;
}

.strip-close {
  flex: none;
  width: 20px;
  height: 20px;
  border: 0;
  border-radius: 5px;
  background: none;
  color: inherit;
  font-size: 11px;
  opacity: 0.55;
  cursor: pointer;
}

.strip-close:hover,
.strip-close:focus-visible {
  background: var(--hover);
  opacity: 1;
}

.strip-tab .label {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.servers-btn {
  flex: none;
  margin: 0 0 4px 8px;
  padding: 4px 10px;
  border: 1px solid color-mix(in srgb, var(--good, #8fe3b0) 45%, transparent);
  border-radius: 8px;
  background: color-mix(in srgb, var(--good, #8fe3b0) 8%, transparent);
  color: var(--good, #8fe3b0);
  font: inherit;
  font-size: 11px;
  letter-spacing: 0.06em;
  cursor: pointer;
}

.servers-btn.alone {
  margin-left: auto;
}

.count {
  flex: none;
  margin-left: auto;
  padding: 0 6px;
  font-size: 11px;
  color: var(--text-dim);
}

.nothing {
  display: flex;
  flex: 1;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 4px;
  padding: 24px;
  text-align: center;
  color: var(--text-dim);
  font-size: 14px;
}

.nothing p {
  margin: 0;
  max-width: 420px;
}

.nothing .title {
  color: var(--text);
  font-size: 17px;
  font-weight: 600;
}

.nothing .start {
  justify-self: start;
}

.remove-sheet {
  position: absolute;
  inset: 0;
  z-index: 30;
  display: grid;
  place-items: center;
  padding: 16px;
  border-radius: 14px;
  background: rgba(6, 4, 20, 0.42);
  -webkit-backdrop-filter: blur(3px); backdrop-filter: blur(3px);
}

.remove-card {
  width: min(420px, 100%);
  padding: 20px 22px;
  border: 1px solid var(--line);
  border-radius: 14px;
  background: var(--panel-background);
  -webkit-backdrop-filter: blur(var(--glass-blur)); backdrop-filter: blur(var(--glass-blur));
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.45);
  box-sizing: border-box;
}

.remove-card h3 {
  margin: 0 0 8px;
  font-size: 16px;
  font-weight: 600;
}

.remove-card p {
  margin: 0 0 16px;
  font-size: 13px;
  line-height: 1.5;
  color: var(--text-dim);
  overflow-wrap: anywhere;
}

.remove-buttons {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 8px;
}

.remove-buttons button {
  padding: 6px 14px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--hover);
  color: var(--text);
  font: inherit;
  cursor: pointer;
}

.remove-buttons .primary {
  background: var(--accent);
  border-color: var(--accent);
  color: #07142e;
  font-weight: 600;
}
</style>
