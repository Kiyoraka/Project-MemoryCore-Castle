<script setup>
// First-run setup, five friendly steps: the memory folder, the CLI, the names, the keys, the
// look. Castle asks for the names itself and writes them into the memory, so the companion
// never has to run MemoryCore's chat wizard. Reopened from Settings with the saved answers.
import { computed, onMounted, reactive, ref } from 'vue'
import {
  avatarGet,
  avatarSet,
  chooseFolder,
  choosePicture,
  clipboardCopy,
  getSettings,
  keySave,
  keyStatus,
  saveSettings,
  setupApply,
  setupCheckCli,
  setupCreate,
  setupFoundKeys,
  setupImportKeys,
  setupInspect,
} from '../api/castle.js'
import { builtInThemes } from '../themes/loader.js'
import { isMac } from '../platform.js'
import { drawsItself, providerLabel } from '../components/providers.js'
import BackupPanel from '../components/BackupPanel.vue'
import DatabasePanel from '../components/DatabasePanel.vue'
import HelpersStatus from '../components/HelpersStatus.vue'
import HelpTip from '../components/HelpTip.vue'

const emit = defineEmits(['done', 'close'])
const props = defineProps({
  // true when opened from Settings (a Close button; the steps start filled in)
  reopened: { type: Boolean, default: false },
})

const STEPS = ['Folder', 'Companion app', 'Names', 'Keys', 'Look', 'Backup']
// The Look step saves everything; Backup comes after, because it works on the saved folder.
const LAST_FORM_STEP = 4
const STYLES = ['A friendly partner', 'A calm mentor', 'A cheerful helper', 'A focused work assistant', 'A caring companion']
const KEY_HELP = {
  openai: 'Draws pictures (ChatGPT image). Not needed with Codex - it draws by itself.',
  seedance: 'Makes short videos (BytePlus ModelArk Seedance).',
  suno: 'Sings songs from lyrics (sunoapi.org).',
  elevenlabs: 'Gives your companion a speaking voice (ElevenLabs).',
}

const step = ref(0)
const busy = ref('')
const problem = ref('')
const settings = ref(null)

const form = reactive({
  folder: '',
  folderState: null,
  // true when the folder came from Create it (the tick shows under that box)
  created: false,
  // true when the names were found in the memory (MemoryCore's own wizard or an earlier setup)
  namesFromMemory: false,
  newParent: '',
  newName: 'My MemoryCore',
  cli: 'claude',
  cliCheck: null,
  companion: '',
  member: '',
  style: STYLES[0],
  customStyle: '',
  avatar: null,
  keys: [],
  // keys found in the memory folder's .env and not yet kept by Castle
  foundKeys: [],
  keyDrafts: {},
  theme: 'constellation',
  startWithWindows: false,
  startSessionOnLaunch: true,
})

const themes = builtInThemes()
const styleText = computed(() => (form.style === 'custom' ? form.customStyle.trim() : form.style))
const shownKeys = computed(() =>
  form.keys.filter((k) => KEY_HELP[k.name] && !(k.name === 'openai' && drawsItself(form.cli))),
)
// the keys step's (?): where the keys are kept and who gets them
const keysTip = computed(
  () =>
    `Keys are ${isMac ? "kept in your Mac's Keychain" : 'kept encrypted by Windows'} and only handed to ${form.companion || 'your companion'}'s session. Without a key, Castle saves the prompt or lyrics instead and tells you.`,
)

const canNext = computed(() => {
  if (step.value === 0) return !!form.folderState?.isMemorycore
  if (step.value === 1) return !!form.cliCheck?.installed
  if (step.value === 2) return form.companion.trim() && form.member.trim() && styleText.value
  return true
})

async function attempt(label, work) {
  problem.value = ''
  busy.value = label
  try {
    return await work()
  } catch (reason) {
    problem.value = String(reason)
    return null
  } finally {
    busy.value = ''
  }
}

// ---------- Step 1: the folder ----------

// Names a memory already holds fill the boxes, so an existing member is never asked twice.
function useFoundNames(state) {
  if (!state) return
  if (state.companionName && !form.companion) form.companion = state.companionName
  if (state.memberName && !form.member) form.member = state.memberName
  if (state.relationshipStyle) {
    if (STYLES.includes(state.relationshipStyle)) form.style = state.relationshipStyle
    else {
      form.style = 'custom'
      form.customStyle = state.relationshipStyle
    }
  }
  form.namesFromMemory = !state.needsNames && !!state.companionName && !!state.memberName
}

async function chooseExisting() {
  const folder = await chooseFolder('Choose your MemoryCore folder')
  if (!folder) return
  form.folder = folder
  form.created = false
  form.folderState = await attempt('Looking at the folder…', () => setupInspect(folder))
  if (form.folderState && !form.folderState.isMemorycore) {
    problem.value = 'This folder has no master-memory.md, so it is not a MemoryCore folder. Choose another, or create a new one below.'
  }
  useFoundNames(form.folderState)
}

async function chooseParent() {
  const parent = await chooseFolder('Where should your new MemoryCore folder go?')
  if (parent) form.newParent = parent
}

async function createNew() {
  const name = form.newName.trim().replace(/[<>:"/\\|?*]/g, '')
  if (!form.newParent || !name) return
  // the parent's own separator: \ for a Windows folder, / for a Mac one
  const sep = form.newParent.includes('\\') || /^[A-Za-z]:/.test(form.newParent) ? '\\' : '/'
  const target = `${form.newParent.replace(/[\\/]+$/, '')}${sep}${name}`
  const state = await attempt('Downloading MemoryCore…', () => setupCreate(target))
  if (state) {
    form.folder = target
    form.folderState = state
    form.created = true
    form.namesFromMemory = false
  }
}

// ---------- Step 2: the CLI ----------

async function checkCli() {
  form.cliCheck = await attempt(`Looking for ${providerLabel(form.cli)}…`, () => setupCheckCli(form.cli))
}

function pickCli(cli) {
  if (form.cli === cli && form.cliCheck) return
  form.cli = cli
  form.cliCheck = null
  checkCli()
}

const copied = ref(false)
async function copyInstall() {
  await clipboardCopy(form.cliCheck.installCommand).catch(() => {})
  copied.value = true
  setTimeout(() => (copied.value = false), 2000)
}

// ---------- Step 3: names and picture ----------

async function choosePortrait() {
  const path = await choosePicture(`Choose a picture for ${form.companion || 'your companion'}`)
  if (!path) return
  form.avatar = await attempt('Saving the picture…', () => avatarSet(path))
}

// ---------- Step 4: keys ----------

// Keys a member already keeps in MemoryCore's .env are offered, never asked for again.
async function loadFoundKeys() {
  if (!form.folder) return
  try {
    const kept = new Set(form.keys.filter((k) => k.set).map((k) => k.name))
    form.foundKeys = (await setupFoundKeys(form.folder)).filter((k) => !kept.has(k.name))
  } catch {
    form.foundKeys = []
  }
}

async function importFoundKeys() {
  const keys = await attempt('Keeping the keys safe…', () => setupImportKeys(form.folder, form.foundKeys.map((k) => k.name)))
  if (keys) {
    form.keys = keys
    form.foundKeys = []
  }
}

async function saveKey(name) {
  const value = (form.keyDrafts[name] ?? '').trim()
  if (!value) return
  const keys = await attempt('Keeping the key safe…', () => keySave(name, value))
  if (keys) {
    form.keys = keys
    form.keyDrafts[name] = ''
  }
}

// ---------- Finish ----------

async function finish() {
  const done = await attempt(`Getting ${form.companion} ready…`, async () => {
    await setupApply(form.folder, form.companion, form.member, styleText.value)
    const next = {
      ...settings.value,
      memoryFolder: form.folder,
      cli: form.cli,
      companionName: form.companion.trim(),
      memberName: form.member.trim(),
      relationshipStyle: styleText.value,
      theme: form.theme,
      startWithWindows: form.startWithWindows,
      startSessionOnLaunch: form.startSessionOnLaunch,
      setupDone: true,
    }
    return saveSettings(next)
  })
  // the memory is saved: the optional backup step follows
  if (done) {
    step.value = LAST_FORM_STEP + 1
    problem.value = ''
  }
}

function next() {
  if (!canNext.value) return
  if (step.value === LAST_FORM_STEP) return finish()
  if (step.value > LAST_FORM_STEP) return emit('done')
  step.value += 1
  problem.value = ''
  if (step.value === 1 && !form.cliCheck) checkCli()
  if (step.value === 3) loadFoundKeys()
}

onMounted(async () => {
  try {
    const view = await getSettings()
    settings.value = view.settings
    const s = view.settings
    form.cli = s.cli ?? 'claude'
    form.companion = s.companionName ?? ''
    form.member = s.memberName ?? ''
    if (s.relationshipStyle) {
      if (STYLES.includes(s.relationshipStyle)) form.style = s.relationshipStyle
      else {
        form.style = 'custom'
        form.customStyle = s.relationshipStyle
      }
    }
    form.theme = s.theme ?? 'constellation'
    form.startWithWindows = s.startWithWindows
    form.startSessionOnLaunch = s.startSessionOnLaunch
    if (s.memoryFolder) {
      form.folder = s.memoryFolder
      form.folderState = await setupInspect(s.memoryFolder)
      useFoundNames(form.folderState)
    }
    form.keys = await keyStatus()
    form.avatar = await avatarGet()
  } catch {
    // Outside Castle's window: an empty form.
  }
})
</script>

<template>
  <div class="setup-backdrop">
    <section class="setup" aria-label="Set up MemoryCore Castle">
      <header>
        <p class="eyebrow">MEMORYCORE CASTLE</p>
        <h1>{{ reopened ? 'Setup' : 'Welcome to your Castle' }}</h1>
        <ol class="steps">
          <li v-for="(name, i) in STEPS" :key="name" :class="{ now: i === step, done: i < step }">{{ name }}</li>
        </ol>
        <button v-if="reopened" class="close" aria-label="Close setup" @click="emit('close')">✕</button>
      </header>

      <!-- 1. Folder -->
      <div v-if="step === 0" class="body">
        <h2>Where does your companion's memory live?</h2>
        <p class="hint">Your MemoryCore folder holds everything your companion remembers. It stays on your computer.</p>
        <div class="choice">
          <h3>I already have one</h3>
          <button class="primary" @click="chooseExisting">Choose my MemoryCore folder</button>
          <p v-if="form.folderState?.isMemorycore && !form.created" class="good">✓ {{ form.folder }}</p>
        </div>
        <div class="choice">
          <h3 class="with-tip">I'm new to MemoryCore <HelpTip text="Castle downloads a fresh MemoryCore for you." /></h3>
          <div class="row">
            <button @click="chooseParent">{{ form.newParent ? 'Change place' : 'Choose where' }}</button>
            <input v-model="form.newName" aria-label="New folder name" />
            <button class="primary" :disabled="!form.newParent || !form.newName.trim()" @click="createNew">Create it</button>
          </div>
          <p v-if="form.created && form.folderState?.isMemorycore" class="good">✓ Created {{ form.folder }}</p>
          <p v-else-if="form.newParent" class="hint">{{ form.newParent }}\{{ form.newName }}</p>
        </div>
      </div>

      <!-- 2. CLI -->
      <div v-else-if="step === 1" class="body">
        <h2>Which app does your companion think with?</h2>
        <p class="hint">It signs in with your own account - Castle never sees your password.</p>
        <div class="cards">
          <button class="card" :class="{ picked: form.cli === 'claude' }" @click="pickCli('claude')">
            <strong>Claude Code</strong><span>Your Claude subscription. Pictures need a ChatGPT key.</span>
          </button>
          <button class="card" :class="{ picked: form.cli === 'codex' }" @click="pickCli('codex')">
            <strong>Codex</strong><span>Your ChatGPT account. Draws pictures by itself.</span>
          </button>
        </div>
        <div v-if="form.cliCheck" class="check">
          <p v-if="form.cliCheck.installed" class="good">✓ {{ form.cliCheck.label }} is installed{{ form.cliCheck.version ? ` (${form.cliCheck.version})` : '' }}.</p>
          <template v-else>
            <p class="warn">{{ form.cliCheck.label }} is not installed yet. Install it with Node.js, then press Check again:</p>
            <div class="row"><code>{{ form.cliCheck.installCommand }}</code><button @click="copyInstall">{{ copied ? 'Copied ✓' : 'Copy' }}</button></div>
          </template>
          <p v-if="form.cliCheck.installed && form.cliCheck.signedIn === false" class="warn">Not signed in yet. {{ form.cliCheck.signInHint }}</p>
          <p v-else-if="form.cliCheck.signedIn" class="good">✓ Signed in.</p>
          <button @click="checkCli">Check again</button>
        </div>
        <HelpersStatus />
      </div>

      <!-- 3. Names -->
      <div v-else-if="step === 2" class="body">
        <h2>Who are you two?</h2>
        <p v-if="form.namesFromMemory" class="good">
          ✓ Found in your memory - press Next to keep them. Changing a name here only changes what Castle shows; ask {{ form.companion }} to update the memory if you rename.
        </p>
        <div class="names">
          <label>Your companion's name <input v-model="form.companion" placeholder="Lyra" maxlength="40" /></label>
          <label>Your name <input v-model="form.member" placeholder="Rin" maxlength="40" /></label>
        </div>
        <p class="label">{{ form.companion || 'Your companion' }} will be…</p>
        <div class="styles">
          <button v-for="s in STYLES" :key="s" :class="{ picked: form.style === s }" @click="form.style = s">{{ s }}</button>
          <button :class="{ picked: form.style === 'custom' }" @click="form.style = 'custom'">My own words</button>
        </div>
        <input v-if="form.style === 'custom'" v-model="form.customStyle" placeholder="a gentle study buddy who teases me a little" />
        <div class="portrait">
          <div class="avatar">
            <img v-if="form.avatar" :src="form.avatar" alt="" />
            <span v-else class="crystal">◆</span>
          </div>
          <div>
            <p class="label with-tip">A picture (optional) <HelpTip text="Without one, your companion is the glowing crystal. You can draw a look later in APPEARANCE." /></p>
            <button @click="choosePortrait">Choose a picture</button>
          </div>
        </div>
      </div>

      <!-- 4. Keys -->
      <div v-else-if="step === 3" class="body">
        <h2 class="with-tip">
          Extra powers (optional)
          <HelpTip :text="keysTip" />
        </h2>
        <div v-if="form.foundKeys.length" class="found">
          <p class="good">✓ Found in your MemoryCore's .env: {{ form.foundKeys.map((k) => `${k.variable} (${k.masked})`).join(', ') }}</p>
          <button class="primary" @click="importFoundKeys">Keep them encrypted in Castle</button>
          <HelpTip text="Your .env stays as it is - you can delete it yourself once Castle keeps the keys." />
        </div>
        <p class="hint">Skip any you don't have.</p>
        <div v-for="key in shownKeys" :key="key.name" class="key">
          <div>
            <strong>{{ key.label }}</strong>
            <p class="hint">{{ KEY_HELP[key.name] }}</p>
          </div>
          <span v-if="key.set" class="good">✓ {{ key.masked }}</span>
          <div class="row">
            <input v-model="form.keyDrafts[key.name]" type="password" :placeholder="key.set ? 'Replace key' : 'Paste key'" autocomplete="off" />
            <button :disabled="!(form.keyDrafts[key.name] ?? '').trim()" @click="saveKey(key.name)">Save</button>
          </div>
        </div>
        <p v-if="form.cli === 'codex'" class="hint">The ChatGPT key is hidden: Codex draws pictures with your ChatGPT account.</p>
      </div>

      <!-- 6. Backup (after everything is saved) -->
      <div v-else-if="step > LAST_FORM_STEP" class="body">
        <h2>Keep the memory safe (optional)</h2>
        <BackupPanel :companion="form.companion" />
        <h2>A database for the memory (optional)</h2>
        <DatabasePanel :companion="form.companion" />
      </div>

      <!-- 5. Look -->
      <div v-else class="body">
        <h2>Make it yours</h2>
        <p class="label">Theme</p>
        <div class="styles">
          <button v-for="t in themes" :key="t.id" :class="{ picked: form.theme === t.id }" @click="form.theme = t.id">{{ t.name }}</button>
        </div>
        <label class="toggle"><input v-model="form.startWithWindows" type="checkbox" /> {{ isMac ? 'Open Castle when you log in' : 'Start Castle when Windows starts' }}</label>
        <label class="toggle"><input v-model="form.startSessionOnLaunch" type="checkbox" /> Wake {{ form.companion || 'your companion' }} when Castle opens</label>
      </div>

      <footer>
        <p v-if="busy" class="busy">{{ busy }}</p>
        <p v-else-if="problem" class="problem">{{ problem }}</p>
        <span class="spacer"></span>
        <button v-if="step > 0 && step <= LAST_FORM_STEP" :disabled="!!busy" @click="step -= 1; problem = ''">Back</button>
        <button v-if="step > LAST_FORM_STEP" @click="emit('done')">Skip for now</button>
        <button v-if="step === 3" :disabled="!!busy" @click="next">Skip</button>
        <button class="primary" :disabled="!canNext || !!busy" @click="next">
          {{ step > LAST_FORM_STEP ? `Wake ${form.companion || 'my companion'}` : step === LAST_FORM_STEP ? 'Save and continue' : 'Next' }}
        </button>
      </footer>
    </section>
  </div>
</template>

<style scoped>
.setup-backdrop {
  position: fixed;
  inset: 0;
  z-index: 50;
  display: grid;
  place-items: center;
  background: rgba(2, 6, 20, 0.55);
  -webkit-backdrop-filter: blur(6px); backdrop-filter: blur(6px);
}
.setup {
  position: relative;
  display: flex;
  flex-direction: column;
  width: min(760px, 92vw);
  max-height: 88vh;
  padding: 28px 32px 22px;
  border: 1px solid var(--line);
  border-radius: 18px;
  background: var(--panel-background);
  -webkit-backdrop-filter: blur(var(--glass-blur)); backdrop-filter: blur(var(--glass-blur));
  box-sizing: border-box;
}
.eyebrow {
  margin: 0;
  font-size: 11px;
  letter-spacing: 0.3em;
  color: var(--accent);
}
h1 {
  margin: 4px 0 14px;
  font-size: 26px;
  font-weight: 500;
}
h2 {
  margin: 0 0 6px;
  font-size: 18px;
  font-weight: 600;
}
h3 {
  margin: 0 0 8px;
  font-size: 14px;
}
.steps {
  display: flex;
  gap: 18px;
  margin: 0 0 18px;
  padding: 0;
  list-style: none;
  font-size: 12px;
  letter-spacing: 0.08em;
  color: var(--text-dim);
}
.steps li.now {
  color: var(--text);
  border-bottom: 2px solid var(--accent);
}
.steps li.done {
  color: var(--good);
}
.close {
  position: absolute;
  top: 18px;
  right: 18px;
}
.body {
  overflow-y: auto;
  padding-right: 4px;
}
.hint {
  margin: 4px 0 10px;
  font-size: 13px;
  color: var(--text-dim);
}
.label {
  margin: 14px 0 6px;
  font-size: 13px;
}
/* a heading or label with its (?) beside it */
h2.with-tip,
h3.with-tip,
.label.with-tip {
  display: flex;
  align-items: center;
}
.choice {
  margin: 12px 0;
  padding: 14px 16px;
  border: 1px solid var(--line);
  border-radius: 12px;
}
.row {
  display: flex;
  gap: 8px;
  align-items: center;
}
.cards {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 12px;
  margin: 12px 0;
}
.card {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 16px;
  text-align: left;
}
.card span {
  font-size: 12px;
  color: var(--text-dim);
}
.picked {
  border-color: var(--accent) !important;
  background: var(--hover) !important;
}
.check {
  margin-top: 8px;
}
code {
  padding: 6px 10px;
  border-radius: 6px;
  background: rgba(0, 0, 0, 0.35);
  font-size: 13px;
}
.names {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 14px;
}
label {
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 13px;
}
.styles {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  margin-bottom: 10px;
}
.portrait {
  display: flex;
  gap: 16px;
  align-items: center;
  margin-top: 16px;
}
.avatar {
  display: grid;
  place-items: center;
  width: 88px;
  height: 88px;
  overflow: hidden;
  border: 2px solid var(--accent);
  border-radius: 50%;
  background: radial-gradient(circle, #1d3b8a, #060d24);
}
.avatar img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.crystal {
  font-size: 40px;
  color: #bcd4ff;
  text-shadow: 0 0 18px #6f9cff;
}
.found {
  margin: 8px 0 14px;
  padding: 12px 14px;
  border: 1px solid var(--good);
  border-radius: 12px;
}
.key {
  display: grid;
  grid-template-columns: 1fr auto;
  gap: 4px 12px;
  align-items: center;
  margin: 10px 0;
  padding: 12px 14px;
  border: 1px solid var(--line);
  border-radius: 12px;
}
.key .row {
  grid-column: 1 / -1;
}
.toggle {
  flex-direction: row;
  align-items: center;
  gap: 10px;
  margin: 12px 0;
  font-size: 14px;
}
input:not([type='checkbox']) {
  flex: 1;
  padding: 8px 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 14px;
}
footer {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: 18px;
  padding-top: 14px;
  border-top: 1px solid var(--line);
}
.spacer {
  flex: 1;
}
.busy {
  margin: 0;
  font-size: 13px;
  color: var(--accent);
}
.problem,
.warn {
  margin: 0 0 6px;
  font-size: 13px;
  color: var(--warn);
}
.good {
  margin: 6px 0;
  font-size: 13px;
  color: var(--good);
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
</style>
