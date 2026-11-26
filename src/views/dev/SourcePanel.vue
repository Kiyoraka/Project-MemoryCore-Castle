<script setup>
import GlassSelect from '../../components/GlassSelect.vue'
import { computed, nextTick, onBeforeUnmount, ref, watch } from 'vue'
import { devGitCommit, devGitDiff, devGitLog, devGitPush, devGitRemote, devGitShow, devGitStatus } from '../../api/devgit.js'
import { devConnect, devGitInit, devGithub } from '../../api/devnew.js'
import { backupSignIn, devState, openUrl } from '../../api/castle.js'
import CommitCard from './CommitCard.vue'
import DiffView from './DiffView.vue'
import { ASK_TO_COMMIT, LOG_PAGE, ago, badges, canPush, codeOf, commitPaths, isNotRepo, nextTicks, pushQuestion, sourceCard, withNewestPage } from './git.js'
import { folderName, tabForSlot } from './devtabs.js'
import { cleanRepoName, githubLine, validRepoName } from './newproject.js'
import { isMac, shortcutLabel } from '../../platform.js'

// A DEV tab's Source panel: the changed files (each with its diff), the member's commit of the
// ticked files, a button that asks the companion to commit instead, Push with one question first
// (never forced), and the commit graph whose commits open as cards. It reads git every 10 seconds
// while it shows, so the companion's own commits appear by themselves. A folder that is not a git
// repo offers Start git here first; a repo with no origin offers the Connect to GitHub card.
const props = defineProps({
  // the DEV tab's slot
  slot: { type: Number, required: true },
  // the companion's name, for the ask button
  companion: { type: String, default: 'your companion' },
  // the panel is showing (it reads git only then)
  shown: { type: Boolean, default: false },
  // the tab's project folder (the Connect card's default repo name); read from the tabs when empty
  folder: { type: String, default: '' },
})
// `ask`: words for the parent to type into this tab's session
const emit = defineEmits(['ask'])

const READ_EVERY_MS = 10000

const status = ref(null)
const commits = ref([])
const more = ref(false)
const ticked = ref(new Set())
const message = ref('')
const problem = ref('')
const notice = ref('')
const working = ref(false)
const askingPush = ref(false)
const cancelPush = ref(null)
const remote = ref(null)
const diff = ref(null)
const card = ref(null)
// dev_git_remote came back at least once (the Connect card waits for it)
const remoteRead = ref(false)
// the Connect to GitHub card: dev_github's answer and the member's choices
const github = ref(null)
const checking = ref(false)
const connectOwner = ref('')
const connectName = ref('')
const connectPrivate = ref(true)
let timer
let reading = false

const files = computed(() => status.value?.files ?? [])
const tickedCount = computed(() => files.value.filter((file) => ticked.value.has(file.path)).length)
const allTicked = computed(() => files.value.length > 0 && tickedCount.value === files.value.length)
const pushReady = computed(() => !working.value && canPush(status.value))
const view = computed(() => sourceCard({ status: status.value, problem: problem.value, remote: remote.value, remoteRead: remoteRead.value, commits: commits.value }))
const ghLine = computed(() => githubLine(github.value))
const owners = computed(() => github.value?.owners ?? [])
const connectNameBad = computed(() => !validRepoName(connectName.value.trim()))
const connectReady = computed(() => !working.value && ghLine.value === 'ready' && Boolean(connectOwner.value) && !connectNameBad.value)

async function refresh() {
  if (reading) return
  reading = true
  try {
    const fresh = await devGitStatus(props.slot)
    ticked.value = nextTicks(status.value?.files, ticked.value, fresh.files)
    status.value = fresh
    const first = await devGitLog(props.slot, 0)
    // older pages the member already opened stay; only the newest page is read again
    const kept = commits.value.length > LOG_PAGE ? withNewestPage(first, commits.value) : null
    if (kept) commits.value = kept
    else {
      commits.value = first
      more.value = first.length === LOG_PAGE
    }
    problem.value = ''
  } catch (reason) {
    problem.value = String(reason)
    // the folder is (no longer) a repo: nothing of git stays on show
    if (isNotRepo(reason)) {
      status.value = null
      commits.value = []
    }
  } finally {
    reading = false
  }
}

async function readRemote() {
  remote.value = await devGitRemote(props.slot).catch(() => null)
  remoteRead.value = true
}

async function readGithub() {
  checking.value = true
  try {
    github.value = await devGithub()
  } catch {
    github.value = { installed: false, signedIn: false, login: null, owners: [] }
  } finally {
    checking.value = false
  }
  if (!owners.value.includes(connectOwner.value)) connectOwner.value = owners.value[0] ?? ''
}

// the Connect card's first look: who is signed in, and the folder's own name as the repo name
watch(view, async (now) => {
  if (now !== 'connect') return
  if (!github.value) readGithub()
  if (connectName.value) return
  let folder = props.folder
  if (!folder) folder = tabForSlot((await devState().catch(() => null))?.tabs, props.slot)?.folder ?? ''
  if (!connectName.value) connectName.value = cleanRepoName(folderName(folder))
})

async function startGit() {
  if (working.value) return
  working.value = true
  notice.value = ''
  try {
    await devGitInit(props.slot)
    problem.value = ''
    notice.value = 'Git started here.'
    await refresh()
    await readRemote()
  } catch (reason) {
    problem.value = String(reason)
  } finally {
    working.value = false
  }
}

async function connect() {
  if (!connectReady.value) return
  working.value = true
  notice.value = ''
  try {
    const url = await devConnect(props.slot, connectOwner.value, connectName.value.trim(), connectPrivate.value)
    problem.value = ''
    notice.value = url ? `Connected to ${url}` : 'Connected to GitHub.'
    await refresh()
    await readRemote()
  } catch (reason) {
    problem.value = String(reason)
  } finally {
    working.value = false
  }
}

async function signIn() {
  try {
    await backupSignIn()
  } catch (reason) {
    problem.value = String(reason)
  }
}

async function loadMore() {
  try {
    const next = await devGitLog(props.slot, commits.value.length)
    commits.value = [...commits.value, ...next]
    more.value = next.length === LOG_PAGE
  } catch (reason) {
    problem.value = String(reason)
  }
}

function toggle(path) {
  const next = new Set(ticked.value)
  if (next.has(path)) next.delete(path)
  else next.add(path)
  ticked.value = next
}

function toggleAll() {
  ticked.value = allTicked.value ? new Set() : new Set(files.value.map((file) => file.path))
}

async function openDiff(path, hash = null) {
  try {
    const text = await devGitDiff(props.slot, path, hash)
    diff.value = { title: hash ? `${path} @ ${hash.slice(0, 7)}` : path, text }
  } catch (reason) {
    problem.value = String(reason)
  }
}

async function commit() {
  if (!message.value.trim() || !tickedCount.value || working.value) return
  working.value = true
  notice.value = ''
  try {
    await devGitCommit(props.slot, commitPaths(files.value, ticked.value), message.value)
    message.value = ''
    notice.value = 'Committed.'
    problem.value = ''
    await refresh()
  } catch (reason) {
    problem.value = String(reason)
  } finally {
    working.value = false
  }
}

function askToCommit() {
  emit('ask', ASK_TO_COMMIT)
  notice.value = `Asked ${props.companion} to commit. The list updates when the commit lands.`
}

// Push asks once, in the page; the safe answer has the focus.
async function askPush() {
  askingPush.value = true
  await nextTick()
  cancelPush.value?.focus()
}

async function push() {
  askingPush.value = false
  working.value = true
  notice.value = ''
  try {
    notice.value = await devGitPush(props.slot)
    problem.value = ''
    await refresh()
  } catch (reason) {
    problem.value = String(reason)
  } finally {
    working.value = false
  }
}

async function openCard(item) {
  try {
    card.value = await devGitShow(props.slot, item.hash)
  } catch (reason) {
    problem.value = String(reason)
  }
}

// The commit box grows with the message up to about eight lines, then scrolls inside.
const box = ref(null)
function grow() {
  const el = box.value
  if (!el) return
  el.style.height = 'auto'
  el.style.height = `${Math.min(Math.max(el.scrollHeight, 72), 176)}px`
}
watch(message, (text) => {
  if (!text && box.value) box.value.style.height = ''
})

// The commit keys as the hint names them: Ctrl+Enter, or ⌘↩ on a Mac.
const commitKeys = shortcutLabel('Ctrl+Enter')

function onKey(event) {
  if (event.key === 'Enter' && (event.ctrlKey || (isMac && event.metaKey))) {
    event.preventDefault()
    commit()
  }
}

watch(
  () => props.shown,
  async (on) => {
    clearInterval(timer)
    if (!on) return
    timer = setInterval(refresh, READ_EVERY_MS)
    await refresh()
    if (remote.value === null && status.value) await readRemote()
  },
  { immediate: true },
)
onBeforeUnmount(() => clearInterval(timer))
defineExpose({ refresh })
</script>

<template>
  <aside class="source-panel" aria-label="Source control">
    <header>
      <span class="tag">SOURCE</span>
      <span v-if="status" class="branch" :title="status.upstream ? `Tracking ${status.upstream}` : 'No upstream yet'">{{ status.branch }}</span>
      <span v-if="status?.upstream" class="sync" :title="`${status.ahead} to push, ${status.behind} to pull`">&#8593;{{ status.ahead }} &#8595;{{ status.behind }}</span>
      <span class="spacer"></span>
      <button v-if="view === 'normal'" class="small" :disabled="!pushReady" :title="status?.upstream ? `Push to ${status.upstream}` : 'Push and set the upstream'" @click="askPush">
        Push{{ status?.ahead ? ` ↑${status.ahead}` : '' }}
      </button>
      <button class="small" title="Read again" aria-label="Read again" @click="refresh">&#8635;</button>
    </header>

    <div class="scroll">
      <div v-if="askingPush" class="confirm" role="group" aria-label="Push" @keydown.esc.stop="askingPush = false">
        <p>{{ pushQuestion(status) }}</p>
        <div class="row">
          <button class="small primary" @click="push">Push</button>
          <button ref="cancelPush" class="small" @click="askingPush = false">Cancel</button>
        </div>
      </div>
      <p v-if="problem && view !== 'init'" class="problem" role="alert">{{ problem }}</p>
      <p v-if="notice" class="notice">{{ notice }}</p>
      <p v-if="!status && !problem" class="quiet">Reading git…</p>

      <div v-if="view === 'init'" class="start-card">
        <p>This folder is not a git repository yet.</p>
        <div class="row">
          <button class="small primary" :disabled="working" @click="startGit">{{ working ? 'Starting…' : 'Start git here' }}</button>
        </div>
      </div>

      <div v-if="view === 'connect'" class="start-card" role="group" aria-label="Connect to GitHub">
        <h4>Connect to GitHub</h4>
        <p class="quiet">This repo has no origin yet. Make one on GitHub and push to it.</p>
        <p v-if="ghLine === 'asking'" class="quiet">Asking GitHub…</p>
        <template v-else-if="ghLine === 'install'">
          <p>The GitHub CLI is not on this computer.</p>
          <div class="row">
            <button class="link" @click="openUrl('https://cli.github.com')">Install the GitHub CLI</button>
            <button class="small" :disabled="checking" @click="readGithub">{{ checking ? 'Checking…' : 'Check again' }}</button>
          </div>
        </template>
        <template v-else-if="ghLine === 'signin'">
          <p>Not signed in to GitHub.</p>
          <div class="row">
            <button class="small primary" @click="signIn">Sign in to GitHub</button>
            <button class="small" :disabled="checking" @click="readGithub">{{ checking ? 'Checking…' : 'Check again' }}</button>
          </div>
        </template>
        <template v-else>
          <label class="field">
            <span>Owner</span>
            <GlassSelect v-model="connectOwner" :options="owners" :disabled="working || !owners.length" aria-label="Owner" />
          </label>
          <label class="field">
            <span>Name</span>
            <input v-model="connectName" autocomplete="off" spellcheck="false" :disabled="working" :aria-invalid="connectNameBad" />
          </label>
          <p v-if="connectName.trim() && connectNameBad" class="hint">Letters, digits, "-", "_" and "." only, and not starting with ".".</p>
          <label class="check">
            <input v-model="connectPrivate" type="checkbox" :disabled="working" />
            Private
          </label>
          <div class="row">
            <button class="small primary" :disabled="!connectReady" @click="connect">{{ working ? 'Creating…' : 'Create and push' }}</button>
          </div>
        </template>
      </div>

      <template v-if="status">
        <div class="section-head">
          <h4>Changes ({{ files.length }})</h4>
          <button v-if="files.length" class="link" @click="toggleAll">{{ allTicked ? 'Untick all' : 'Tick all' }}</button>
        </div>
        <ul class="changes">
          <li v-for="file in files" :key="file.path" :title="`${codeOf(file.code).word}${file.from ? ` from ${file.from}` : ''}`">
            <input type="checkbox" :checked="ticked.has(file.path)" :aria-label="`Include ${file.path}`" @change="toggle(file.path)" />
            <span class="code" :class="codeOf(file.code).tone">{{ codeOf(file.code).letter }}</span>
            <span class="path">{{ file.path }}</span>
            <button class="link" :aria-label="`Diff of ${file.path}`" @click="openDiff(file.path)">diff</button>
          </li>
        </ul>
        <p v-if="!files.length" class="quiet">Nothing to commit. Every file is as it was last committed.</p>

        <textarea
          ref="box"
          v-model="message"
          class="message"
          rows="3"
          aria-label="Commit message"
          :placeholder="`Message (${commitKeys} commits on ${status.branch || 'the branch'})`"
          @keydown="onKey"
          @input="grow"
        ></textarea>
        <div class="row">
          <button class="small primary" :disabled="working || !message.trim() || !tickedCount" @click="commit">
            Commit{{ tickedCount && tickedCount !== files.length ? ` ${tickedCount}` : '' }}
          </button>
          <button class="small" :disabled="!files.length" :title="`${companion} writes the message and commits in the session`" @click="askToCommit">Ask {{ companion }} to commit</button>
        </div>

        <div class="section-head">
          <h4>Graph</h4>
        </div>
        <p v-if="!commits.length" class="quiet">No commits yet.</p>
        <ul class="graph">
          <li v-for="item in commits" :key="item.hash" :class="{ merge: item.parents.length > 1 }" :title="`${item.short} by ${item.author}, ${ago(item.date)}`">
            <button class="commit" @click="openCard(item)">
              <span class="lane" aria-hidden="true"></span>
              <span class="subject">{{ item.subject }}</span>
              <span v-for="badge in badges(item.refs)" :key="badge.text" class="badge" :class="badge.kind">{{ badge.text }}</span>
            </button>
          </li>
        </ul>
        <button v-if="more && commits.length" class="link more" @click="loadMore">Show older commits</button>
      </template>
    </div>

    <DiffView v-if="diff" :title="diff.title" :text="diff.text" @close="diff = null" />
    <!-- the card steps aside while one of its diffs is open, so Esc closes the diff first -->
    <CommitCard v-if="card && !diff" :detail="card" :remote="remote" @diff="(path) => openDiff(path, card.commit.hash)" @close="card = null" />
  </aside>
</template>

<style scoped>
.source-panel {
  display: flex;
  flex-direction: column;
  min-width: 0;
  min-height: 0;
  padding: 14px 10px 14px 16px;
  background: var(--panel-background);
  border: 1px solid var(--line);
  border-radius: 14px;
  -webkit-backdrop-filter: blur(var(--glass-blur)); backdrop-filter: blur(var(--glass-blur));
  box-sizing: border-box;
  font-size: 13px;
}

header {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 6px 8px 0;
  border-bottom: 1px solid var(--line);
}

.tag {
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.2em;
  color: var(--accent);
}

.branch {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.sync {
  flex: none;
  font-size: 12px;
  color: var(--text-dim);
}

.spacer {
  flex: 1;
}

.scroll {
  display: flex;
  flex-direction: column;
  gap: 6px;
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 8px 6px 0 0;
  scrollbar-width: var(--scrollbar-width);
  scrollbar-color: var(--scrollbar-thumb) var(--scrollbar-track);
}

/* the column scrolls as a whole: nothing in it is ever squeezed */
.scroll > * {
  flex: none;
}

.section-head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  margin-top: 8px;
}

h4 {
  margin: 0;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--text-dim);
}

.quiet {
  margin: 4px 0;
  color: var(--text-dim);
}

.problem {
  margin: 0;
  color: var(--warn);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.notice {
  margin: 0;
  font-size: 12px;
  color: var(--good);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.confirm {
  padding: 10px;
  border: 1px solid var(--accent);
  border-radius: 10px;
  background: var(--hover);
}

.confirm p {
  margin: 0 0 8px;
  overflow-wrap: anywhere;
}

.row {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

ul {
  margin: 0;
  padding: 0;
  list-style: none;
}

.changes li {
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 2px 0;
}

.changes input {
  flex: none;
  margin: 0;
  accent-color: var(--accent);
}

.code {
  flex: none;
  width: 20px;
  font-family: 'Cascadia Mono', Consolas, ui-monospace, Menlo, monospace;
  font-size: 12px;
  text-align: center;
}

.code.new {
  color: var(--good);
}

.code.gone,
.code.conflict {
  color: var(--warn);
}

.code.changed,
.code.moved {
  color: var(--accent);
}

/* a long path keeps its file name in sight and loses the start of the folder */
.path {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  direction: rtl;
  text-align: left;
}

.message {
  min-height: 72px;
  max-height: 176px;
  margin-top: 6px;
  padding: 9px 12px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: rgba(0, 0, 0, 0.22);
  color: var(--text);
  font: inherit;
  font-size: 13.5px;
  line-height: 1.4;
  resize: none;
  box-sizing: border-box;
}

.message:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

.graph li {
  position: relative;
}

.commit {
  position: relative;
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  padding: 3px 4px 3px 20px;
  border: 0;
  border-radius: 6px;
  background: none;
  color: var(--text);
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.commit:hover {
  background: var(--hover);
}

.commit:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}

.lane {
  position: absolute;
  left: 6px;
  top: 0;
  bottom: 0;
  width: 2px;
  background: color-mix(in srgb, var(--accent) 45%, transparent);
}

.lane::after {
  content: '';
  position: absolute;
  left: -3px;
  top: calc(50% - 4px);
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: var(--accent);
}

.merge .lane::after {
  background: var(--good);
}

.subject {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.badge {
  flex: none;
  padding: 0 7px;
  border: 1px solid var(--line);
  border-radius: 999px;
  font-size: 11px;
  white-space: nowrap;
}

.badge.branch {
  border-color: var(--accent);
  color: var(--accent);
}

.badge.tag {
  border-color: var(--good);
  color: var(--good);
}

.small {
  padding: 4px 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}

.small.primary {
  border-color: var(--accent);
  background: var(--accent);
  color: #07142e;
  font-weight: 600;
}

.small:hover:not(:disabled) {
  filter: brightness(1.12);
  background-color: var(--hover);
}

.small.primary:hover:not(:disabled) {
  background-color: var(--accent);
}

.small:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.small:focus-visible,
.link:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

.link {
  flex: none;
  padding: 0;
  border: 0;
  background: none;
  color: var(--accent);
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}

.link:hover {
  text-decoration: underline;
}

.more {
  align-self: flex-start;
  margin: 4px 0 8px;
}

.start-card {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px;
  border: 1px solid var(--accent);
  border-radius: 10px;
  background: var(--hover);
}

.start-card p {
  margin: 0;
  overflow-wrap: anywhere;
}

.field {
  display: flex;
  align-items: center;
  gap: 8px;
}

.field > span {
  width: 46px;
  flex: none;
  color: var(--text-dim);
}

/* text boxes in the theme's own colours (the dropdowns beside them are GlassSelect) */
.field input:not([type='checkbox']) {
  flex: 1;
  min-width: 0;
  box-sizing: border-box;
  padding: 7px 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--widget-background);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

.field input::placeholder {
  color: var(--text-dim);
  opacity: 0.7;
}

.field input[aria-invalid='true'] {
  border-color: var(--warn);
}

.field input:focus-visible {
  outline: none;
  border-color: var(--accent);
}

.check {
  display: flex;
  align-items: center;
  gap: 6px;
}

.check input {
  margin: 0;
  accent-color: var(--accent);
}

.hint {
  margin: 0;
  font-size: 12px;
  color: var(--warn);
}
</style>
