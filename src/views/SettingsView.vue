<script setup>
// Settings: everything setup asked, changeable any time - the memory folder and companion, the
// keys (masked, Show, Test, Remove), quick actions, the room, backup, startup and updates.
import { computed, onMounted, ref } from 'vue'
import ModalSheet from '../components/ModalSheet.vue'
import BackupPanel from '../components/BackupPanel.vue'
import DatabasePanel from '../components/DatabasePanel.vue'
import { DIARY_LOOKS, lookOf } from '../components/diaryviews.js'
import { PROFILE_LOOKS, profileLookOf } from './profile/profile.js'
import {
  providerSwitch,
  roomsApply,
  roomCompanionRefresh,
  linkView,
  avatarClear,
  avatarGet,
  avatarSet,
  chooseFolder,
  choosePicture,
  getSettings,
  keyForget,
  keyReveal,
  keySave,
  keyStatus,
  keyTest,
  openUrl,
  releasesPage,
  saveSettings,
  setupApply,
  setupCheckCli,
  setupInspect,
  voiceStudioStatus,
} from '../api/castle.js'

const props = defineProps({
  // which section opens first ("keys" from a toast or a STUDIO card)
  section: { type: String, default: 'companion' },
  version: { type: String, default: '' },
  updateLine: { type: String, default: '' },
})
const emit = defineEmits(['close', 'saved', 'setup', 'whatsnew'])

const SECTIONS = [
  { id: 'companion', label: 'Companion' },
  { id: 'keys', label: 'Keys' },
  { id: 'quick', label: 'Quick actions' },
  { id: 'rooms', label: 'Rooms' },
  { id: 'database', label: 'Database' },
  { id: 'backup', label: 'Backup' },
  { id: 'startup', label: 'Start-up' },
  { id: 'about', label: 'About' },
]

const tab = ref(props.section)
const settings = ref(null)
const view = ref(null)
const keys = ref([])
const drafts = ref({})
const shown = ref({})
const tests = ref({})
const avatar = ref(null)
const cliCheck = ref(null)
const busy = ref('')
const problem = ref('')
const done = ref('')
const newAction = ref({ label: '', command: '' })
const link = ref(null)

async function readLink() {
  link.value = await linkView().catch(() => null)
}

async function switchLink(patch) {
  await save(patch, 'Saved.')
  await readLink()
}

// The rooms that can be switched off (HOME always stays).
const ROOM_SWITCHES = [
  { id: 'theme', label: 'THEME' },
  { id: 'apps', label: 'APPS' },
  { id: 'diary', label: 'DIARY' },
  { id: 'projects', label: 'PROJECTS' },
  { id: 'library', label: 'LIBRARY' },
  { id: 'appearance', label: 'APPEARANCE' },
  { id: 'studio', label: 'STUDIO' },
  { id: 'story', label: 'STORY' },
  { id: 'world', label: 'WORLD' },
  { id: 'chat', label: 'CHAT' },
  { id: 'dev', label: 'DEV' },
]

async function switchRoom(id, on) {
  await attempt('Saving…', async () => {
    const rooms = await roomsApply({ ...settings.value.rooms, [id]: on })
    settings.value = { ...settings.value, rooms }
    done.value = on ? `${id.toUpperCase()} is back.` : `${id.toUpperCase()} is turned off - nothing was removed.`
    emit('saved')
  })
}

// the room sign-in and the companion's room key are made by CHAT itself, never shown or pasted here
const shownKeys = computed(() => keys.value.filter((k) => k.name !== 'member' && !k.name.startsWith('room') && !(k.name === 'openai' && settings.value?.cli === 'codex')))

async function attempt(label, work) {
  problem.value = ''
  done.value = ''
  busy.value = label
  try {
    return await work()
  } catch (reason) {
    problem.value = String(reason?.message ?? reason)
    return null
  } finally {
    busy.value = ''
  }
}

// VoiceStudio (optional, for people who run it on this computer): is it answering right now?
const voiceUp = ref(null)
async function checkVoice() {
  voiceUp.value = settings.value?.voicestudioOn ? await voiceStudioStatus().catch(() => false) : null
}
async function toggleVoice(on) {
  const saved = await save({ voicestudioOn: on }, on ? "Saved. The Voice card in STUDIO now uses VoiceStudio while it is running." : 'Saved.')
  if (saved) await checkVoice()
}

async function save(patch, message = 'Saved.') {
  const next = { ...settings.value, ...patch }
  const saved = await attempt('Saving…', () => saveSettings(next))
  if (saved) {
    view.value = saved
    settings.value = saved.settings
    done.value = message
    emit('saved')
  }
  return saved
}

// ---------- Companion ----------

async function changeFolder() {
  const folder = await chooseFolder('Choose your MemoryCore folder')
  if (!folder) return
  const state = await attempt('Looking at the folder…', () => setupInspect(folder))
  if (!state) return
  if (!state.isMemorycore) {
    problem.value = 'That folder has no master-memory.md, so it is not a MemoryCore folder.'
    return
  }
  await save({ memoryFolder: folder }, 'The memory folder is changed. Wake your companion again to use it.')
}

async function saveNames() {
  const s = settings.value
  if (!s.companionName?.trim() || !s.memberName?.trim()) {
    problem.value = 'Both names are needed.'
    return
  }
  const saved = await save({}, 'Names saved.')
  if (saved && s.memoryFolder) await attempt('Updating CLAUDE.md and AGENTS.md…', () => setupApply(s.memoryFolder, s.companionName, s.memberName, s.relationshipStyle ?? ''))
  if (saved) await tellRoom('Names saved.')
}

// When the companion is in the chat room, its name and picture there follow Settings.
async function tellRoom(message) {
  const told = await attempt('Telling the room…', roomCompanionRefresh)
  if (told?.inRoom) done.value = `${message} ${told.renamed ? 'The room shows the new name and picture.' : 'The room shows it too.'}`
}

async function switchCli(cli) {
  if (settings.value.cli === cli) return
  cliCheck.value = await attempt('Checking…', () => setupCheckCli(cli))
  await attempt('Queuing switch…', async () => {
    const state = await providerSwitch(cli)
    done.value = state.message
  })
}

async function changeAvatar() {
  const path = await choosePicture('Choose a picture for your companion')
  if (!path) return
  const saved = await attempt('Saving…', () => avatarSet(path))
  if (saved) {
    avatar.value = saved
    await tellRoom('Picture saved.')
  }
  emit('saved')
}

async function clearAvatar() {
  await attempt('Saving…', avatarClear)
  avatar.value = await avatarGet()
  await tellRoom('Picture cleared.')
  emit('saved')
}

// ---------- Keys ----------

async function saveKey(name) {
  const value = (drafts.value[name] ?? '').trim()
  if (!value) return
  const next = await attempt('Keeping the key safe…', () => keySave(name, value))
  if (next) {
    keys.value = next
    drafts.value[name] = ''
    shown.value[name] = ''
    done.value = 'Key kept. Wake your companion again to use it.'
  }
}

async function toggleShow(name) {
  shown.value[name] = shown.value[name] ? '' : (await keyReveal(name)) ?? ''
}

async function test(name) {
  tests.value[name] = { ok: null, message: 'Checking…' }
  tests.value[name] = (await attempt('', () => keyTest(name))) ?? { ok: false, message: problem.value }
}

async function remove(name) {
  const next = await attempt('Removing…', () => keyForget(name))
  if (next) keys.value = next
}

// ---------- Quick actions ----------

async function addAction() {
  const a = { label: newAction.value.label.trim(), command: newAction.value.command.trim() }
  if (!a.label || !a.command) return
  if (await save({ quickActions: [...(settings.value.quickActions ?? []), a] }, 'Button added.')) newAction.value = { label: '', command: '' }
}

async function removeAction(i) {
  const list = [...settings.value.quickActions]
  list.splice(i, 1)
  await save({ quickActions: list }, 'Button removed.')
}

onMounted(async () => {
  try {
    view.value = await getSettings()
    settings.value = view.value.settings
    keys.value = await keyStatus()
    avatar.value = await avatarGet()
    await readLink()
    await checkVoice()
  } catch (reason) {
    problem.value = String(reason)
  }
})
</script>

<template>
  <ModalSheet tag="SETTINGS" title="Settings" @close="emit('close')">
    <div v-if="settings" class="settings">
      <nav class="tabs">
        <button v-for="s in SECTIONS" :key="s.id" :class="{ active: tab === s.id }" @click="tab = s.id">{{ s.label }}</button>
      </nav>

      <section v-if="tab === 'companion'" class="pane">
        <h3>Memory folder</h3>
        <div class="row"><code>{{ settings.memoryFolder ?? 'Not chosen' }}</code><button @click="changeFolder">Change</button></div>
        <h3>Thinks with</h3>
        <div class="row">
          <button :class="{ primary: settings.cli === 'claude' }" @click="switchCli('claude')">Claude Code</button>
          <button :class="{ primary: settings.cli === 'codex' }" @click="switchCli('codex')">Codex</button>
          <span v-if="cliCheck && !cliCheck.installed" class="warn">{{ cliCheck.label }} is not installed: <code>{{ cliCheck.installCommand }}</code></span>
        </div>
        <label v-if="settings.cli === 'claude'" class="only-this">
          <input type="checkbox" :checked="!!settings.onlyThisCompanion" @change="save({ onlyThisCompanion: $event.target.checked }, 'Saved. It takes effect the next time ' + (settings.companionName ?? 'your companion') + ' wakes.')" />
          <span>
            Only {{ settings.companionName ?? 'this companion' }} in Castle
            <small>Leave out my personal Claude Code setup (its own CLAUDE.md, hooks and plugins) in Castle's session. Turn this on when that setup is another companion, or to test with a clean one.</small>
          </span>
        </label>
        <h3>Names</h3>
        <div class="row">
          <label>Companion <input v-model="settings.companionName" maxlength="40" /></label>
          <label>You <input v-model="settings.memberName" maxlength="40" /></label>
          <label class="wide">Your companion is <input v-model="settings.relationshipStyle" maxlength="80" /></label>
        </div>
        <button class="primary" @click="saveNames">Save names</button>
        <p class="hint">Names already written in your memory files stay as they are - ask your companion to update the memory if you rename.</p>
        <p class="hint">Your name in the chat room is your Foundation name (Library card > Profile). Your companion's name and picture here go to the room when it is let in.</p>
        <h3>Picture</h3>
        <div class="row">
          <div class="avatar"><img v-if="avatar" :src="avatar" alt="" /><span v-else>◆</span></div>
          <button @click="changeAvatar">Choose a picture</button>
          <button @click="clearAvatar">Use the drawn look / crystal</button>
        </div>
        <h3>Usage card</h3>
        <label class="toggle"><input type="checkbox" :checked="settings.showClaudeUsage !== false" @change="save({ showClaudeUsage: $event.target.checked })" /> Show Claude Code usage on HOME</label>
        <label class="toggle"><input type="checkbox" :checked="settings.showCodexUsage !== false" @change="save({ showCodexUsage: $event.target.checked })" /> Show Codex usage on HOME</label>
        <p class="hint">The USAGE card on HOME shows how much of the five-hour and weekly limits is used. One you switch off is not asked at all. Switch both off and the card says so.</p>
        <h3>Local voice (optional)</h3>
        <label class="toggle"><input type="checkbox" :checked="!!settings.voicestudioOn" :disabled="!!busy" @change="toggleVoice($event.target.checked)" /> Use VoiceStudio for the Voice card in STUDIO</label>
        <p v-if="settings.voicestudioOn" class="hint">VoiceStudio is {{ voiceUp === null ? 'being checked…' : voiceUp ? 'running on this computer.' : 'not running. Open it and wait for it to finish starting, then check again.' }} <button v-if="voiceUp === false" @click="checkVoice">Check again</button></p>
        <p class="hint">For people who run their own <a href="#" @click.prevent="openUrl('https://github.com/debpalash/voicestudio')">VoiceStudio</a> on this PC. Off: Castle never contacts it. On: STUDIO's Voice card can speak a line with it, and the clip is saved with your other voice lines. Everything stays on this computer. Castle does not install or start VoiceStudio, and its voice models carry their own licences (some are non-commercial).</p>
        <button @click="emit('setup')">Run the first-run setup again</button>
      </section>

      <section v-else-if="tab === 'keys'" class="pane">
        <p class="hint">Keys are kept encrypted by Windows and only handed to your companion's session. Without a key, your companion saves the idea instead and Castle tells you.</p>
        <div v-for="k in shownKeys" :key="k.name" class="key">
          <div class="row">
            <strong>{{ k.label }}</strong>
            <span v-if="k.set" class="good">{{ shown[k.name] || k.masked }}</span>
            <span v-else class="hint">not set</span>
            <span class="spacer"></span>
            <template v-if="k.set">
              <button @click="toggleShow(k.name)">{{ shown[k.name] ? 'Hide' : 'Show' }}</button>
              <button @click="test(k.name)">Test</button>
              <button class="danger" @click="remove(k.name)">Remove</button>
            </template>
          </div>
          <div class="row">
            <input v-model="drafts[k.name]" type="password" :placeholder="k.set ? 'Paste a new key to replace' : 'Paste key'" autocomplete="off" />
            <button :disabled="!(drafts[k.name] ?? '').trim()" @click="saveKey(k.name)">Save</button>
          </div>
          <p v-if="tests[k.name]" :class="tests[k.name].ok === false ? 'warn' : 'good'">{{ tests[k.name].message }}</p>
        </div>
        <p v-if="settings.cli === 'codex'" class="hint">The ChatGPT key is hidden: Codex draws with your ChatGPT account.</p>
        <p class="hint">Room keys for the MemoryCore Foundation room are set at the CHAT room's door.</p>
      </section>

      <section v-else-if="tab === 'quick'" class="pane">
        <p class="hint">Your own buttons, after Save diary / Save project / Save memory. A button types its words into your companion's session.</p>
        <div v-for="(a, i) in settings.quickActions" :key="a.label + i" class="row">
          <strong>{{ a.label }}</strong><code>{{ a.command }}</code><span class="spacer"></span>
          <button class="danger" @click="removeAction(i)">Remove</button>
        </div>
        <div class="row">
          <input v-model="newAction.label" placeholder="Plan my day" maxlength="24" />
          <input v-model="newAction.command" class="wide" placeholder="the words to type, e.g. plan my day" maxlength="200" />
          <button class="primary" :disabled="!newAction.label.trim() || !newAction.command.trim()" @click="addAction">Add</button>
        </div>
      </section>

      <section v-else-if="tab === 'database'" class="pane">
        <DatabasePanel :companion="settings.companionName ?? 'your companion'" @changed="emit('saved')" />
      </section>

      <section v-else-if="tab === 'backup'" class="pane">
        <BackupPanel :companion="settings.companionName ?? 'your companion'" />
      </section>

      <section v-else-if="tab === 'rooms'" class="pane">
        <p class="hint">Turn off a room you don't use: its tab goes and nothing runs for it. Nothing is removed - turn it back on and everything is there as it was. HOME always stays.</p>
        <div class="rooms">
          <label v-for="r in ROOM_SWITCHES" :key="r.id" class="toggle">
            <input type="checkbox" :checked="settings.rooms?.[r.id] !== false" :disabled="!!busy" @change="switchRoom(r.id, $event.target.checked)" /> {{ r.label }}
          </label>
        </div>
        <p class="hint">Turning STORY off ends the story session; turning WORLD off closes the world screens; CHAT stops reading the room. Their files stay.</p>
        <p class="hint">Arrange the tabs your way: drag a tab in the top bar onto another, or press Alt+Left / Alt+Right on a tab. Castle remembers the order.</p>
        <button v-if="settings.tabOrder?.length" :disabled="!!busy" @click="save({ tabOrder: [] }, 'The tabs are back in their first order.')">Put the tabs back in their first order</button>

        <h3>Chat alerts</h3>
        <label class="toggle"><input type="checkbox" :checked="!!settings.alertSound" :disabled="!!busy" @change="save({ alertSound: $event.target.checked })" /> Also play a sound when a new message arrives in the chat room</label>
        <label class="toggle"><input type="checkbox" :checked="settings.alertNotify !== false" :disabled="!!busy" @change="save({ alertNotify: $event.target.checked })" /> Show a desktop notification when a new message arrives in the chat room</label>
        <p class="hint">Only while you are not looking at CHAT. The notification names who wrote and shows the start of the line (worth knowing during a screen share), and Windows' Do Not Disturb and quiet hours apply to it. The sound is Windows' own notification sound and starts switched off, because it plays even in quiet hours. Your own messages, and your own companion's, never alert. Castle checks the room every 30 seconds while it is hidden in the tray, so an alert can come up to 30 seconds late. Switch both off to keep only the glow on the CHAT tab.</p>

        <h3>Diary</h3>
        <p class="hint">How the DIARY room is read. The pages are the same; only the design changes. The THEME room shows each one with the page it wears.</p>
        <div class="row">
          <button v-for="l in DIARY_LOOKS" :key="l.id" :class="{ primary: lookOf(settings.diaryLook) === l.id }" :title="l.line" @click="save({ diaryLook: l.id })">{{ l.label }}</button>
        </div>
        <h3>Profile</h3>
        <p class="hint">How HOME's PROFILE tab shows {{ settings.companionName ?? 'your companion' }}'s emotion engine. The traits are the same; only the design changes.</p>
        <div class="row">
          <button v-for="l in PROFILE_LOOKS" :key="l.id" :class="{ primary: profileLookOf(settings.profileLook) === l.id }" :title="l.line" @click="save({ profileLook: l.id })">{{ l.label }}</button>
        </div>
        <h3>Neural Link</h3>
        <label class="toggle"><input type="checkbox" :checked="settings.linkOn !== false" :disabled="!!busy" @change="switchLink({ linkOn: $event.target.checked })" /> Neural Link - your terminals leave each other notes in the memory folder's <code>link</code> folder</label>
        <label class="toggle"><input type="checkbox" :checked="!!settings.roomMayPass" :disabled="!!busy || settings.linkOn === false" @change="switchLink({ roomMayPass: $event.target.checked })" /> {{ settings.companionName ?? 'Your companion' }} may pass my own words from the chat room to my workspace</label>
        <p class="hint">Only lines you post yourself can be passed on - never a guest's. Your workspace answers in a note, and your companion says it in the room in its own words. Nothing in the link folder leaves this computer.</p>
        <div v-if="link?.on" class="link-list">
          <p v-if="!link.here.length" class="hint">No terminal is here right now.</p>
          <p v-for="h in link.here" :key="h.terminal" class="hint"><strong>{{ h.terminal }}</strong><span v-if="h.place"> - {{ h.place }}</span> (seen {{ h.seen }})</p>
          <p v-if="!link.notes.length" class="hint">No notes are waiting.</p>
          <p v-for="(n, i) in link.notes" :key="i" class="hint">{{ n.from }} &rarr; {{ n.to }}: {{ n.about }}<span v-if="!n.wake"> (no knock)</span></p>
          <button @click="readLink">Look again</button>
        </div>
      </section>

      <section v-else-if="tab === 'startup'" class="pane">
        <label class="toggle"><input type="checkbox" :checked="!!settings.displayClock" :disabled="!!busy" @change="save({ displayClock: $event.target.checked })" /> Display clock</label>
        <label class="clock-format">Clock format
          <select :value="settings.clockFormat || '24h'" :disabled="!settings.displayClock || !!busy" @change="save({ clockFormat: $event.target.value })">
            <option value="24h">24-hour (14:30)</option>
            <option value="12h">12-hour (2:30 PM)</option>
          </select>
        </label>
        <p class="hint">Shows your local time in the top-right corner, just before the music controls.</p>
        <label class="toggle"><input type="checkbox" :checked="settings.startWithWindows" @change="save({ startWithWindows: $event.target.checked })" /> Start Castle when Windows starts</label>
        <p v-if="view?.startupNote" class="hint">{{ view.startupNote }}</p>
        <label class="toggle"><input type="checkbox" :checked="settings.startSessionOnLaunch" @change="save({ startSessionOnLaunch: $event.target.checked })" /> Wake {{ settings.companionName ?? 'your companion' }} when Castle starts with Windows</label>
        <label class="toggle"><input type="checkbox" :checked="settings.minimizeToTaskbar" @change="save({ minimizeToTaskbar: $event.target.checked })" /> Minimise to the taskbar instead of the tray</label>
        <p class="hint">The minus button in the top bar then minimises Castle to the taskbar, where one click brings it back. With this off, it hides Castle to the tray icon.</p>
        <p class="hint">Change the look in the THEME room.</p>
      </section>

      <section v-else class="pane">
        <h3>MemoryCore Castle {{ version }}</h3>
        <p class="hint">{{ updateLine || 'Castle looks for a new version every few hours.' }}</p>
        <p class="about-links">
          <button @click="emit('whatsnew')">What's new</button>
          <button @click="releasesPage().then(openUrl).catch(() => {})">All releases</button>
        </p>
        <p class="hint">Free and open source under the MIT license. Change anything - see MAKE-IT-YOURS.md in the source.</p>
        <p class="hint">Castle checks GitHub for new versions, and talks to the MemoryCore Foundation only when you sign in or use CHAT. Your memory, keys and settings stay on this computer (and in your private backup, if you choose).</p>
        <h3>Credits</h3>
        <p class="hint">The Deep Space world - recipe by Ashran Helmi · ashranhelmi.com (Space Playbook, "Nebula &amp; Starfield").</p>
      </section>

      <footer>
        <span v-if="busy" class="busy">{{ busy }}</span>
        <span v-else-if="problem" class="warn">{{ problem }}</span>
        <span v-else-if="done" class="good">{{ done }}</span>
      </footer>
    </div>
  </ModalSheet>
</template>

<style scoped>
.only-this {
  display: flex;
  gap: 10px;
  align-items: flex-start;
  margin-top: 10px;
  font-size: 14px;
}
.only-this small {
  display: block;
  margin-top: 2px;
  font-size: 12px;
  color: var(--text-dim);
}
.rooms {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  gap: 8px 16px;
}
.settings {
  display: flex;
  flex-direction: column;
  gap: 14px;
}
.tabs {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.tabs .active {
  border-color: var(--accent);
  background: var(--hover);
}
.pane {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
h3 {
  margin: 8px 0 0;
  font-size: 13px;
  font-weight: 500;
  letter-spacing: 0.1em;
}
.row {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: center;
}
.spacer {
  flex: 1;
}
label {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 13px;
}
label.wide,
input.wide {
  flex: 1;
}
.clock-format { align-self: flex-start; }
.clock-format select {
  width: auto;
  max-width: 100%;
  min-height: 34px;
  padding: 6px 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--panel-background);
  color: var(--text);
  font: inherit;
}
.clock-format select:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
.clock-format select:disabled { opacity: .45; cursor: not-allowed; }
label.toggle {
  flex-direction: row;
  align-items: center;
  gap: 8px;
  font-size: 14px;
}
input:not([type='checkbox']) {
  padding: 7px 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 14px;
}
.key {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 12px 14px;
  border: 1px solid var(--line);
  border-radius: 12px;
}
.key input {
  flex: 1;
}
code {
  padding: 4px 8px;
  border-radius: 6px;
  background: rgba(0, 0, 0, 0.3);
  font-size: 13px;
}
.avatar {
  display: grid;
  place-items: center;
  width: 56px;
  height: 56px;
  overflow: hidden;
  border: 2px solid var(--accent);
  border-radius: 50%;
  color: #cfe0ff;
  font-size: 24px;
}
.avatar img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.hint {
  margin: 0;
  font-size: 13px;
  color: var(--text-dim);
}
.good {
  margin: 0;
  font-size: 13px;
  color: var(--good);
}
.warn {
  margin: 0;
  font-size: 13px;
  color: var(--warn);
}
.busy {
  font-size: 13px;
  color: var(--accent);
}
footer {
  min-height: 20px;
}
/* Settings > About: What's new and All releases */
.about-links {
  display: flex;
  gap: 8px;
}
button {
  padding: 6px 12px;
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
button.danger {
  border-color: var(--bad);
  color: var(--bad);
}
button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
</style>
