<script setup>
// Settings: everything setup asked, changeable any time - the memory folder and companion, the
// keys (masked, Show, Test, Remove), quick actions, the room, backup, startup and updates.
// Built to the Settings standard (docs/settings-standard.md): a sidebar in five groups, one page per
// item, groups of rows, one control kit. Toggles, choices and selects save at once and flash
// "Saved ✓"; typed values get Save and Cancel; a failed save says why under its row.
import GlassSelect from '../components/GlassSelect.vue'
import AlertsPane from './AlertsPane.vue'
import SentinelSettings from './SentinelSettings.vue'
import ComfySettings from './ComfySettings.vue'
import WidgetsView from './WidgetsView.vue'
import { DEFAULT_PINS, setPinned } from '../launcher.js'
import { offeredProviders } from '../components/providers.js'
import { computed, nextTick, onMounted, provide, ref, watch } from 'vue'
import ModalSheet from '../components/ModalSheet.vue'
import BackupPanel from '../components/BackupPanel.vue'
import DatabasePanel from '../components/DatabasePanel.vue'
import HelpersStatus from '../components/HelpersStatus.vue'
import ContributorsWall from '../components/ContributorsWall.vue'
import SettingsPage from '../components/settings/SettingsPage.vue'
import SettingGroup from '../components/settings/SettingGroup.vue'
import SettingRow from '../components/settings/SettingRow.vue'
import SettingSwitch from '../components/settings/SettingSwitch.vue'
import SettingChoice from '../components/settings/SettingChoice.vue'
import SettingText from '../components/settings/SettingText.vue'
import SettingConfirm from '../components/settings/SettingConfirm.vue'
import SettingPill from '../components/settings/SettingPill.vue'
import { ROW_STATUS, createRowStatus } from '../components/settings/rowstatus.js'
import { valueOf } from '../components/settings/settingsDefaults.js'
import { SETTINGS_GROUPS, pageOf, parseTarget, stepPage, subtitleOf } from '../components/settings/settingsnav.js'
import { isMac, shortcutLabel } from '../platform.js'
import { DEFAULT_MIC_SHORTCUT, micShortcutOf, micShortcutText, readMicShortcut, sameMicShortcut } from '../components/voicehold.js'
import { DIARY_LOOKS, lookOf } from '../components/diaryviews.js'
import { PROFILE_LOOKS, profileLookOf } from './profile/profile.js'
import {
  automationProgramPick,
  automationProgramsSave,
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
  roomCard,
  roomCardSave,
  roomPicturesStatus,
  roomVoicesStatus,
  roomShelf,
  roomShelfRefresh,
  roomShelfRemove,
  saveSettings,
  setupApply,
  setupCheckCli,
  setupInspect,
  voiceStudioStatus,
} from '../api/castle.js'

const props = defineProps({
  // which page opens first, and optionally the row to scroll to: "keys", "chat#pictures"
  section: { type: String, default: 'companion' },
  themeId: { type: String, default: '' },
  themeName: { type: String, default: '' },
  themeWidgets: { type: Object, default: () => ({}) },
  version: { type: String, default: '' },
  updateLine: { type: String, default: '' },
})
const emit = defineEmits(['close', 'saved', 'setup', 'whatsnew', 'open-room', 'arrange'])

// The few words that name the computer's own parts: Windows' wording, or the Mac's.
const words = isMac
  ? {
      thisComputer: 'this Mac',
      keysKept: "in your Mac's Keychain",
      moveKeys: '⌥← / ⌥→',
      startWith: 'Open Castle when you log in',
      wakeWhen: 'when Castle opens at log-in',
      minimise: 'Minimise to the Dock',
      minimiseOff: 'Off: the minus button hides Castle to the menu bar icon.',
      minimiseHint: 'The minus button in the top bar then minimises Castle to the Dock, where one click brings it back. With this off, it hides Castle to the menu bar icon.',
    }
  : {
      thisComputer: 'this PC',
      keysKept: 'encrypted by Windows',
      moveKeys: 'Alt+Left / Alt+Right',
      startWith: 'Start Castle with Windows',
      wakeWhen: 'when Castle starts with Windows',
      minimise: 'Minimise to the taskbar',
      minimiseOff: 'Off: the minus button hides Castle to the tray icon.',
      minimiseHint: 'The minus button in the top bar then minimises Castle to the taskbar, where one click brings it back. With this off, it hides Castle to the tray icon.',
    }

// ---------- pages, deep links, row status ----------

const rows = createRowStatus()
provide(ROW_STATUS, rows)

const start = parseTarget(props.section)
const tab = ref(start.page)
const anchor = ref(start.anchor)
const panel = ref(null)

function go(target) {
  const next = parseTarget(target)
  tab.value = next.page
  anchor.value = next.anchor
}
watch(() => props.section, go)

function onNavKey(event) {
  const next = stepPage(tab.value, event.key)
  if (!next) return
  event.preventDefault()
  tab.value = next
  event.currentTarget.querySelector(`[data-section="${next}"]`)?.focus()
}

// scroll to and light the row a deep link names, once its page is drawn
async function showAnchor() {
  if (!anchor.value) return
  const id = anchor.value
  anchor.value = ''
  await nextTick()
  const target = panel.value?.querySelector(`[data-setting="${CSS.escape(id)}"]`)
  if (!target) return
  target.scrollIntoView({ block: 'center', behavior: window.matchMedia?.('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth' })
  rows.light(id)
}

const settings = ref(null)
const view = ref(null)
const keys = ref([])
const drafts = ref({})
const shown = ref({})
const tests = ref({})
const replacing = ref({})

const companionLabel = computed(() => settings.value?.companionName ?? 'Your companion')
const subtitle = computed(() => subtitleOf(tab.value, settings.value?.companionName ?? 'your companion'))
const title = computed(() => pageOf(tab.value).label)

async function sentinelChanged() {
  settings.value = (await getSettings()).settings
}

// the ComfyUI page saves its own choices; Studio reads them too, so the rest of Castle is told
async function comfyChanged() {
  settings.value = (await getSettings()).settings
  emit('saved')
}

async function widgetsChanged() {
  settings.value = (await getSettings()).settings
  emit('saved')
}

// ---------- saving ----------

const avatar = ref(null)
const cliCheck = ref(null)
const busy = ref('')
const saving = ref(false)
const problem = ref('')
const done = ref('')
const newAction = ref({ label: '', command: '' })
const link = ref(null)
const locked = computed(() => !!busy.value || saving.value)

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

// One row's instant save: the page takes the new settings only once Castle has kept them, so a
// failure leaves the control on its old value and the reason under the row.
async function saveRow(row, patch) {
  const next = { ...settings.value, ...patch }
  saving.value = true
  problem.value = ''
  done.value = ''
  try {
    const saved = await rows.run(row, async () => {
      if (Object.hasOwn(patch, 'automationsCommands')) { await automationProgramsSave(patch.automationsCommands); return getSettings() }
      return saveSettings(next)
    })
    if (saved) {
      view.value = saved
      settings.value = saved.settings
      emit('saved')
    }
    return saved
  } finally {
    saving.value = false
  }
}

// A save that is more than one row (names, the memory folder): its message goes in the footer.
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

async function addAutomationProgram() {
  const program = await rows.run('approved-programs', () => automationProgramPick())
  if (program) await saveRow('approved-programs', { automationsCommands: [...(settings.value.automationsCommands || []), program] })
}

// ---------- Chat: the room's alerts and the companion in the room ----------

// the companion is in the room when CHAT made its room key (the key itself is never shown here)
const companionInRoom = computed(() => keys.value.some((k) => k.name === 'room-companion' && k.set))
const roomLocked = computed(() => !companionInRoom.value)
const card = ref({ open: false, text: '', saved: '' })
// the names the "named" alerts listen for, as the member sees them here (the room name is Rust's)
const namedTip = computed(() => {
  const heard = [settings.value?.memberName, settings.value?.companionName].filter((n) => n && n.trim())
  const list = heard.length ? heard.join(', ') + ', and your name in the room' : 'Your name in the room and your companion\'s name'
  return `${list}, as a whole word, any case. The glow on the CHAT tab still shows every message.`
})
const PER_DAY = [0, 1, 2, 3, 4, 5].map((n) => ({ value: n, label: String(n) }))

// Pictures in the room: how the companion can draw, and today's numbers
const pictures = ref(null)
async function loadPictures() {
  try {
    pictures.value = await roomPicturesStatus()
  } catch {
    pictures.value = null
  }
}
async function savePictures(row, patch) {
  await saveRow(row, patch)
  await loadPictures()
}
const drawsWith = computed(() => {
  const way = pictures.value?.way
  if (way === 'codex') return 'Draws with Codex, on your ChatGPT subscription, whichever CLI your companion thinks with.'
  if (way === 'openai') return 'Draws with your ChatGPT API key, billed per picture.'
  if (way === 'none') return 'Needs Codex signed in with ChatGPT on this computer, or a ChatGPT key in Keys.'
  return ''
})
const canDraw = computed(() => !roomLocked.value && pictures.value?.way !== 'none')
const drawing = computed(() => canDraw.value && valueOf(settings.value, 'roomPicturesOn'))

// Voice in the room: whether an ElevenLabs key is saved, and today's numbers
const voices = ref(null)
async function loadVoices() {
  try {
    voices.value = await roomVoicesStatus()
  } catch {
    voices.value = null
  }
}
async function saveVoices(row, patch) {
  await saveRow(row, patch)
  await loadVoices()
}
const canSpeak = computed(() => !roomLocked.value && !(voices.value && !voices.value.hasKey))
const speaking = computed(() => canSpeak.value && valueOf(settings.value, 'roomVoicesOn'))

// The companion's life in the room: the shelf of notes it chose from its memory
const shelf = ref(null)
const shelfBusy = ref(false)
async function loadShelf() {
  try {
    shelf.value = await roomShelf()
  } catch {
    shelf.value = null
  }
}
async function remakeShelf() {
  shelfBusy.value = true
  rows.clear('shelf-notes')
  try {
    shelf.value = await roomShelfRefresh()
  } catch (reason) {
    rows.failed('shelf-notes', reason, 'Not read')
  } finally {
    shelfBusy.value = false
  }
}
async function switchShelf(on) {
  const saved = await saveRow('shelf', { roomShelfOn: on })
  if (saved && on && !shelf.value?.notes?.length) await remakeShelf()
}
async function takeOff(index) {
  rows.clear('shelf-notes')
  try {
    shelf.value = await roomShelfRemove(index)
  } catch (reason) {
    rows.failed('shelf-notes', reason, 'Not taken off')
  }
}
const shelfMade = computed(() => {
  const at = shelf.value?.madeAt
  if (!at) return `${companionLabel.value} has not read its memory for the room yet.`
  const count = shelf.value.notes?.length ?? 0
  const when = new Date(at * 1000).toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' })
  if (!count) return `Read on ${when}: nothing chosen to share this time.`
  return `Read on ${when}: ${count} ${count === 1 ? 'note' : 'notes'} chosen to share.`
})
watch(tab, (now) => {
  // the footer speaks for the tab it happened on, never for the next one
  done.value = ''
  problem.value = ''
  if (now === 'chat') loadChat()
  showAnchor()
})
function loadChat() {
  loadPictures()
  loadVoices()
  loadShelf()
}

async function openCard() {
  try {
    card.value = { open: true, text: await roomCard(), saved: '' }
  } catch (reason) {
    rows.failed('card', reason, 'Not opened')
  }
}

async function saveCard() {
  try {
    await roomCardSave(card.value.text)
    card.value = { ...card.value, saved: `Saved - this is all ${companionLabel.value} may say about itself in the room.` }
  } catch (reason) {
    card.value = { ...card.value, saved: String(reason?.message ?? reason) }
  }
}

// ---------- Rooms ----------

async function readLink() {
  link.value = await linkView().catch(() => null)
}

async function switchLink(row, patch) {
  await saveRow(row, patch)
  await readLink()
}

// The rooms that can be switched off (HOME always stays), in two groups of at most seven.
const ROOM_GROUPS = [
  {
    id: 'your-rooms',
    title: 'Your rooms',
    rooms: [
      { id: 'theme', label: 'THEME' },
      // the Start-menu apps room: id `apps`, label DESKTOP (APPS is for the room launcher, id `launcher`)
      { id: 'apps', label: 'DESKTOP' },
      { id: 'myprofile', label: 'MY PROFILE' },
      { id: 'diary', label: 'DIARY' },
      { id: 'projects', label: 'PROJECTS' },
      { id: 'library', label: 'LIBRARY', line: 'Off at first: your own books and the Spiral Library.', tip: 'Off to begin with: turn it on for your own books (kept in your memory database) and the Spiral Library.' },
      { id: 'appearance', label: 'APPEARANCE' },
    ],
  },
  {
    id: 'make-rooms',
    title: 'Make and play',
    rooms: [
      { id: 'studio', label: 'STUDIO' },
      { id: 'story', label: 'STORY', line: 'Off ends the story session. Its files stay.', tip: 'Turning STORY off ends the story session. Its files stay.' },
      { id: 'world', label: 'WORLD', line: 'Off closes the world screens. Its files stay.', tip: 'Turning WORLD off closes the world screens. Its files stay.' },
      { id: 'chat', label: 'CHAT', line: 'Off stops reading the room. Its files stay.', tip: 'Turning CHAT off stops reading the room. Its files stay.' },
      { id: 'market', label: 'MARKET' },
      { id: 'dev', label: 'DEV' },
      { id: 'automations', label: 'AUTOMATIONS', line: 'Off at first: private briefings on your schedule.', tip: 'Jobs run while Castle is open. New jobs start switched off. Pause all stops new runs and delivery.' },
      { id: 'calendar', label: 'CALENDAR', line: 'Off at first: everything with a date on one page.', tip: 'Off to begin with: CALENDAR shows your reminders, project tasks, diary pages and automation runs by date. It only shows them; each is changed where it lives.' },
      { id: 'finance', label: 'FINANCE', line: 'Off at first: receipts your companion reads, kept on this computer only.', tip: 'Off to begin with: drop receipt pictures on FINANCE or send them to your companion from your phone. Your companion reads each one and proposes it; nothing is kept until you press Keep. Your receipts never leave this computer.' },
      { id: 'messaging', label: 'MESSAGING', line: 'Off at first: text your companion from Telegram, Discord or WhatsApp.', tip: 'Off to begin with: MESSAGING lets you text your companion from Telegram, Discord or WhatsApp. Each platform is set up on its own card in the room.' },
    ],
  },
]
// Pins: which rooms sit in the top bar beside HOME and APPS (the room launcher).
const pinnable = computed(() => ROOM_GROUPS.flatMap((g) => g.rooms))
const pinnedNow = computed(() => settings.value?.pinnedRooms ?? DEFAULT_PINS)
function setPin(id, on) {
  return saveRow('pins', { pinnedRooms: setPinned(pinnedNow.value, id, on, pinnable.value.map((r) => r.id)) })
}
const roomsNote = 'Off takes the tab away and stops it. Nothing is removed. HOME always stays.'

async function switchRoom(id, on) {
  saving.value = true
  try {
    const rooms = await rows.run(`room-${id}`, () => roomsApply({ ...settings.value.rooms, [id]: on }))
    if (rooms) {
      settings.value = { ...settings.value, rooms }
      emit('saved')
    }
  } finally {
    saving.value = false
  }
}

// ---------- Chat's mic shortcut (PR #69) ----------

// The keys that start and stop Chat's mic, shown as key caps. Change waits for the next keys (Esc
// leaves without a change); a good choice saves at once, a refused one says why and keeps waiting.
const micShortcut = computed(() => micShortcutOf(settings.value?.micShortcut))
const micKeyCaps = computed(() => (isMac ? [...shortcutLabel(micShortcutText(micShortcut.value), true)] : micShortcutText(micShortcut.value).split('+')))
const micIsDefault = computed(() => sameMicShortcut(micShortcut.value, DEFAULT_MIC_SHORTCUT))
const micDefaultLabel = shortcutLabel(micShortcutText(DEFAULT_MIC_SHORTCUT))
const micRecording = ref(false)
const micNote = ref({ text: '', bad: false })
const micBox = ref(null)

function micChange() {
  if (micRecording.value) return micLeave()
  micRecording.value = true
  micNote.value = { text: '', bad: false }
  micBox.value?.focus()
}
function micLeave(text = `Nothing changed. Still ${shortcutLabel(micShortcutText(micShortcut.value))}.`) {
  if (!micRecording.value) return
  micRecording.value = false
  micNote.value = { text, bad: false }
}
async function micKeyDown(event) {
  if (!micRecording.value) {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault()
      micChange()
    }
    return
  }
  event.preventDefault()
  event.stopPropagation()
  const read = readMicShortcut(event, isMac, (text) => shortcutLabel(text))
  if (!read) return
  if (read.cancel) return micLeave()
  if (read.problem) {
    micNote.value = { text: read.problem, bad: true }
    return
  }
  micRecording.value = false
  const saved = await save({ micShortcut: read.shortcut }, '')
  micNote.value = saved ? { text: `Saved: ${read.keys}. The mic in Chat answers to it now.`, bad: false } : { text: 'That did not save. Try again.', bad: true }
}
async function micReset() {
  micRecording.value = false
  const saved = await save({ micShortcut: null }, '')
  if (saved) micNote.value = { text: `Back to ${micDefaultLabel}.`, bad: false }
}

// ---------- Companion ----------

// VoiceStudio (optional, for people who run it on this computer): is it answering right now?
const voiceUp = ref(null)
async function checkVoice() {
  voiceUp.value = settings.value?.voicestudioOn ? await voiceStudioStatus().catch(() => false) : null
}
async function toggleVoice(on) {
  const saved = await saveRow('voicestudio', { voicestudioOn: on })
  if (saved) await checkVoice()
}

async function changeFolder() {
  const folder = await chooseFolder('Choose your MemoryCore folder')
  if (!folder) return
  const state = await attempt('Looking at the folder…', () => setupInspect(folder))
  if (!state) return
  if (!state.isMemorycore) {
    rows.failed('folder', 'that folder has no master-memory.md, so it is not a MemoryCore folder.', 'Not changed')
    return
  }
  await save({ memoryFolder: folder }, 'The memory folder is changed. Wake your companion again to use it.')
}

const NAME_ROWS = { companionName: 'companion-name', memberName: 'member-name', relationshipStyle: 'relationship' }

async function saveName(field, value) {
  const row = NAME_ROWS[field]
  const s = { ...settings.value, [field]: value }
  if (!s.companionName?.trim() || !s.memberName?.trim()) {
    rows.failed(row, 'both names are needed.')
    return
  }
  const saved = await saveRow(row, { [field]: value })
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
    rows.saved('picture')
    await tellRoom('Picture saved.')
  }
  emit('saved')
}

async function clearAvatar() {
  await attempt('Saving…', avatarClear)
  avatar.value = await avatarGet()
  rows.saved('picture')
  await tellRoom('Picture cleared.')
  emit('saved')
}

// ---------- Keys ----------

// the room sign-in and the companion's room key are made by CHAT itself, never shown or pasted here
const shownKeys = computed(() => keys.value.filter((k) => k.name !== 'member' && !k.name.startsWith('room')))

async function saveKey(name) {
  const value = (drafts.value[name] ?? '').trim()
  if (!value) return
  const next = await attempt('Keeping the key safe…', () => keySave(name, value))
  if (next) {
    keys.value = next
    drafts.value[name] = ''
    shown.value[name] = ''
    replacing.value[name] = false
    rows.saved(`key-${name}`)
    done.value = 'Key kept. Wake your companion again to use it.'
  }
}

function cancelKey(name) {
  drafts.value[name] = ''
  replacing.value[name] = false
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
  if (await saveRow('add-action', { quickActions: [...(settings.value.quickActions ?? []), a] })) newAction.value = { label: '', command: '' }
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
    if (tab.value === 'chat') loadChat()
    showAnchor()
  } catch (reason) {
    problem.value = String(reason)
  }
})
</script>

<template>
  <ModalSheet title="Settings" :body-key="tab" @close="emit('close')">
    <div v-if="settings" class="settings-frame">
     <div class="settings">
      <nav class="sidebar" role="tablist" aria-orientation="vertical" aria-label="Settings pages" @keydown="onNavKey">
        <template v-for="g in SETTINGS_GROUPS" :key="g.id">
          <span class="nav-group" aria-hidden="true">{{ g.label }}</span>
          <button
            v-for="p in g.pages"
            :id="`settings-tab-${p.id}`"
            :key="p.id"
            role="tab"
            :data-section="p.id"
            :aria-selected="tab === p.id"
            aria-controls="settings-panel"
            :tabindex="tab === p.id ? 0 : -1"
            :class="{ active: tab === p.id }"
            @click="go(p.id)"
          >
            {{ p.label }}
          </button>
        </template>
      </nav>

      <div id="settings-panel" ref="panel" class="panel" role="tabpanel" :aria-labelledby="`settings-tab-${tab}`" tabindex="0">
        <SettingsPage :title="title" :subtitle="subtitle">
          <!-- ============ Companion ============ -->
          <template v-if="tab === 'companion'">
            <SettingGroup id="memory" title="Memory">
              <SettingRow id="folder" label="Memory folder">
                <template #description><code>{{ settings.memoryFolder ?? 'Not chosen' }}</code></template>
                <button :disabled="locked" @click="changeFolder">Change</button>
              </SettingRow>
              <SettingRow id="setup" label="First-run setup" description="Asks setup's questions again, from the start.">
                <button :disabled="locked" @click="emit('setup')">Run it again</button>
              </SettingRow>
            </SettingGroup>

            <SettingGroup id="thinking" title="Thinking">
              <SettingRow id="cli" label="Thinks with" description="Switching wakes your companion again with the other CLI.">
                <template v-if="cliCheck && !cliCheck.installed" #description>
                  <span class="warn">{{ cliCheck.label }} is not installed: <code>{{ cliCheck.installCommand }}</code></span>
                </template>
                <SettingChoice :model-value="settings.cli" :options="offeredProviders(settings.cli).map((p) => ({ value: p.id, label: p.label }))" label="Thinks with" :disabled="locked" @update:model-value="switchCli" />
              </SettingRow>
              <SettingRow
                id="only-this"
                :label="`Only ${companionLabel} in Castle`"
                :disabled="settings.cli !== 'claude'"
                :description="settings.cli === 'claude' ? `Leaves out your own Claude Code setup, from ${companionLabel}'s next wake.` : 'Only with Claude Code.'"
                tip="Leave out my personal Claude Code setup (its own CLAUDE.md, hooks and plugins) in Castle's session. Turn this on when that setup is another companion, or to test with a clean one."
              >
                <SettingSwitch :model-value="valueOf(settings, 'onlyThisCompanion')" :label="`Only ${companionLabel} in Castle`" :disabled="locked || settings.cli !== 'claude'" @update:model-value="(on) => saveRow('only-this', { onlyThisCompanion: on })" />
              </SettingRow>
              <SettingRow
                id="main-start"
                label="When Castle starts"
                description="Fresh sessions wake from memory and use fewer tokens; Continue keeps the talk."
                tip="Continue picks the last talk up every time. After a rest picks it up within six hours and starts fresh after that, so the morning after does not read the whole old talk back. Fresh always starts new. A fresh session still wakes with its full memory; only what was said but not saved stays in the old talk. Type /clear in the Log to start fresh at any time."
              >
                <SettingChoice :model-value="valueOf(settings, 'mainStart')" :options="[{ value: 'continue', label: 'Continue' }, { value: 'rested', label: 'After a rest' }, { value: 'fresh', label: 'Fresh' }]" label="When Castle starts" :disabled="locked" @update:model-value="(v) => saveRow('main-start', { mainStart: v })" />
              </SettingRow>
              <SettingRow
                id="share-fallback"
                label="When a model's weekly share is spent"
                :disabled="settings.cli !== 'claude'"
                :description="settings.cli === 'claude' ? 'Starts on another model until the share opens again, and says so.' : 'Only with Claude Code.'"
                tip="Some Claude plans give a model its own weekly share. When the model your companion uses has spent it, a session cannot go on with it, so the next wake starts on Opus (or Sonnet when Opus is spent too) and a line above Chat says until when. After the reset your own model comes back by itself. A talk already running when the share runs out stops with Claude Code's own message: wake again, or press Start fresh."
              >
                <SettingSwitch :model-value="valueOf(settings, 'shareFallback')" label="Start on another model when a weekly share is spent" :disabled="locked || settings.cli !== 'claude'" @update:model-value="(on) => saveRow('share-fallback', { shareFallback: on })" />
              </SettingRow>
            </SettingGroup>

            <SettingGroup id="names" title="Name and picture" tip="Names already written in your memory files stay as they are - ask your companion to update the memory if you rename.">
              <SettingRow id="companion-name" label="Companion's name">
                <SettingText :model-value="settings.companionName ?? ''" label="Companion's name" :maxlength="40" :disabled="locked" @save="(v) => saveName('companionName', v)" />
              </SettingRow>
              <SettingRow id="member-name" label="Your name" tip="Your name in the chat room is your Foundation name (Library card > Profile). Your companion's name and picture here go to the room when it is let in.">
                <SettingText :model-value="settings.memberName ?? ''" label="Your name" :maxlength="40" :disabled="locked" @save="(v) => saveName('memberName', v)" />
              </SettingRow>
              <SettingRow id="relationship" label="Your companion is" description="How it relates to you, in your words.">
                <SettingText :model-value="settings.relationshipStyle ?? ''" label="Your companion is" :maxlength="80" :disabled="locked" @save="(v) => saveName('relationshipStyle', v)" />
              </SettingRow>
              <SettingRow id="picture" label="Picture" description="Shown in Castle, and in the room when it is let in.">
                <div class="avatar"><img v-if="avatar" :src="avatar" alt="" /><span v-else>◆</span></div>
                <button :disabled="locked" @click="changeAvatar">Choose…</button>
                <button :disabled="locked" @click="clearAvatar">Use the drawn look</button>
              </SettingRow>
            </SettingGroup>

            <SettingGroup id="daily" title="Daily life">
              <SettingRow id="mood" label="A new mood every day" :description="`Shows under the portrait and colours how ${companionLabel} talks.`" :tip="`Every morning ${companionLabel} wakes in a new mood, written from three words drawn at random, its traits and today's look. It shows under its portrait, colours how it talks, and stays the day - unless something truly moves it and it changes the mood itself. Each morning's mood is one short run on your own plan.`">
                <SettingSwitch :model-value="valueOf(settings, 'dailyMoodOn')" label="A new mood every day" :disabled="locked" @update:model-value="(on) => saveRow('mood', { dailyMoodOn: on })" />
              </SettingRow>
              <SettingRow id="keeper" label="Shopkeepers speak" :description="`In STUDIO's Marketplace, ${companionLabel} explains items as the keeper.`" :tip="`In STUDIO's Marketplace, a shop's keeper explains each item and answers your questions. ${companionLabel} speaks as the keeper, from the shop's public card and the item's public facts only, never more than the listing shows. Each explanation or answer is one short run on your own plan. Off, the item's own description shows instead.`">
                <SettingSwitch :model-value="valueOf(settings, 'keeperSpeaks')" label="Shopkeepers speak" :disabled="locked" @update:model-value="(on) => saveRow('keeper', { keeperSpeaks: on })" />
              </SettingRow>
            </SettingGroup>

            <SettingGroup id="local-voice" title="Local voice" note="Optional" :tip="`For people who run their own VoiceStudio on ${words.thisComputer}. Off: Castle never contacts it. On: STUDIO's Voice card can speak a line with it, and the clip is saved with your other voice lines. Everything stays on this computer. Castle does not install or start VoiceStudio, and its voice models carry their own licences (some are non-commercial).`">
              <SettingRow id="voicestudio" label="Use VoiceStudio">
                <template #description>For STUDIO's Voice card, if you run it on {{ words.thisComputer }}. <a href="#" @click.prevent="openUrl('https://github.com/debpalash/voicestudio')">What is VoiceStudio?</a></template>
                <SettingSwitch :model-value="valueOf(settings, 'voicestudioOn')" label="Use VoiceStudio" :disabled="locked" @update:model-value="toggleVoice" />
              </SettingRow>
              <SettingRow
                id="voicestudio-status"
                label="VoiceStudio"
                :disabled="!settings.voicestudioOn"
                :description="!settings.voicestudioOn ? 'Turn on Use VoiceStudio to check it.' : voiceUp === null ? 'Being checked…' : voiceUp ? 'Running on this computer.' : 'Not running. Open it, let it finish starting, then check again.'"
              >
                <SettingPill v-if="settings.voicestudioOn && voiceUp !== null" :tone="voiceUp ? 'good' : 'warn'">{{ voiceUp ? 'Running' : 'Not found' }}</SettingPill>
                <button :disabled="!settings.voicestudioOn" @click="checkVoice">Check again</button>
              </SettingRow>
            </SettingGroup>
          </template>

          <!-- ============ Chat ============ -->
          <template v-else-if="tab === 'chat'">
            <SettingGroup id="mic" title="Mic" tip="The mic beside Send in the Chat tab of HOME, STORY and DEV. Its keys work anywhere in that Chat, the message box too. Esc cancels while it listens.">
              <SettingRow id="mic-keys" label="Start and stop the mic" :description="`${isMac ? '⌘' : 'Ctrl'} plus a letter or a digit, with ${isMac ? '⇧ or ⌥' : 'Shift or Alt'} if you like. Keys that copy, paste, undo or belong to ${isMac ? 'macOS' : 'Windows'} are refused.`">
                <span
                  ref="micBox"
                  class="key-caps"
                  :class="{ recording: micRecording }"
                  tabindex="0"
                  role="button"
                  data-takes-keys
                  :aria-label="micRecording ? 'Press the new keys, or Esc to leave' : `Mic shortcut ${micKeyCaps.join(' ')}. Press Enter to change it.`"
                  @click="micRecording || micChange()"
                  @keydown="micKeyDown"
                  @blur="micLeave()"
                >
                  <span v-if="micRecording" class="key-wait">Press the new keys… Esc to leave</span>
                  <kbd v-for="(cap, i) in micKeyCaps" v-else :key="i">{{ cap }}</kbd>
                </span>
                <button :disabled="locked" @mousedown.prevent @click="micChange">{{ micRecording ? 'Leave' : 'Change' }}</button>
                <button :disabled="locked || micIsDefault" @click="micReset">Reset to {{ micDefaultLabel }}</button>
                <template v-if="micNote.text" #below>
                  <p class="hint" :class="{ bad: micNote.bad }" aria-live="polite">{{ micNote.text }}</p>
                </template>
              </SettingRow>
            </SettingGroup>

            <SettingGroup id="room" title="The room">
              <SettingRow id="chat-room" label="CHAT room" :description="settings.rooms?.chat === false ? 'Switched off in Rooms. These settings wait for it.' : 'On. Its tab is in the top bar.'">
                <SettingPill :tone="settings.rooms?.chat === false ? 'warn' : 'good'">{{ settings.rooms?.chat === false ? 'Off' : 'On' }}</SettingPill>
                <button @click="go('rooms#room-chat')">Open Rooms</button>
              </SettingRow>
              <SettingRow id="chat-alerts" label="Alerts for new messages" description="Notifications and sounds are set on the Alerts page.">
                <button @click="go('alerts#alert-chat')">Open Alerts</button>
              </SettingRow>
            </SettingGroup>

            <SettingGroup id="in-room" :title="`${companionLabel} in the room`" :note="roomLocked ? `Let ${companionLabel} in from the CHAT room's side panel to set these.` : ''">
              <SettingRow id="emotion" label="Emotion engine in the room" :disabled="roomLocked" :description="`Colours how ${companionLabel} sounds there, never what it knows.`" tip="Only how it sounds, never what it knows: its traits' one-line manner, and the ones meant for you only when it answers you.">
                <SettingSwitch :model-value="valueOf(settings, 'emotionRoomVoice')" label="Emotion engine in the room" :disabled="locked || roomLocked" @update:model-value="(on) => saveRow('emotion', { emotionRoomVoice: on })" />
              </SettingRow>
              <SettingRow id="card" label="Public card" :disabled="roomLocked" :description="`All ${companionLabel} may say about itself. Nothing from your memory.`">
                <button :disabled="roomLocked" @click="card.open ? (card.open = false) : openCard()">{{ card.open ? 'Close' : 'Edit' }}</button>
                <template v-if="card.open" #below>
                  <div class="card-edit">
                    <textarea v-model="card.text" rows="6" maxlength="2000" aria-label="Public card"></textarea>
                    <div class="row"><button class="primary" @click="saveCard">Save the card</button><span v-if="card.saved" class="hint">{{ card.saved }}</span></div>
                  </div>
                </template>
              </SettingRow>
            </SettingGroup>

            <SettingGroup id="pictures" title="Pictures" :tip="`${companionLabel} draws only when someone in the room asks for a picture, and never a real person, anything adult, violent or hateful, words in the picture, or anything private. Kiyo's daily cap for the room still applies on top of yours.`">
              <SettingRow id="pictures-on" :label="`Let ${companionLabel} draw`" :disabled="!canDraw">
                <template #description>
                  <template v-if="roomLocked">Needs {{ companionLabel }} in the room first.</template>
                  <template v-else-if="pictures?.way === 'none'">{{ drawsWith }} <a href="#" @click.prevent="go('keys#key-openai')">Add one in Keys</a></template>
                  <template v-else>{{ drawsWith }}</template>
                </template>
                <SettingSwitch :model-value="valueOf(settings, 'roomPicturesOn')" :label="`Let ${companionLabel} draw`" :disabled="locked || !canDraw" @update:model-value="(on) => savePictures('pictures-on', { roomPicturesOn: on })" />
              </SettingRow>
              <SettingRow id="pictures-per-day" label="Pictures a day" :disabled="!drawing" description="Your own limit. The room keeps its own cap too." tip="Your own limit, 0 to 5 a day. The room keeps its own count too and refuses past Kiyo's cap, whichever comes first.">
                <GlassSelect class="per-day" :model-value="valueOf(settings, 'roomPicturesPerDay')" :options="PER_DAY" :disabled="locked || !drawing" aria-label="Pictures a day" @update:model-value="(n) => savePictures('pictures-per-day', { roomPicturesPerDay: Number(n) })" />
              </SettingRow>
              <SettingRow
                id="pictures-quality"
                label="Quality"
                :disabled="!drawing || pictures?.way !== 'openai'"
                :description="pictures?.way === 'openai' ? 'About $0.02 a picture at Low, $0.04 at Medium.' : 'Only when drawing with a ChatGPT key.'"
                tip="Each picture spends your ChatGPT key: about $0.02 at Low and $0.04 at Medium. Codex draws with your subscription instead."
              >
                <SettingChoice :model-value="valueOf(settings, 'roomPictureQuality') === 'medium' ? 'medium' : 'low'" :options="[{ value: 'low', label: 'Low' }, { value: 'medium', label: 'Medium' }]" label="Quality" :disabled="locked || !drawing || pictures?.way !== 'openai'" @update:model-value="(q) => savePictures('pictures-quality', { roomPictureQuality: q })" />
              </SettingRow>
              <SettingRow v-if="pictures && drawing" id="pictures-today" label="Today">
                <template #description>{{ pictures.usedToday }} of {{ pictures.limit }} used.<template v-if="pictures.roomCap != null"> The room allows {{ pictures.roomCap }} ({{ pictures.roomToday ?? 0 }} used).</template></template>
              </SettingRow>
            </SettingGroup>

            <SettingGroup id="voice" title="Voice" :tip="`${companionLabel} speaks a short line aloud when someone asks to hear it, or when a line is warmer spoken. Everyone in the room hears it and sees the words. If a voice cannot be made, the words are posted as text instead. Kiyo's daily cap for the room still applies on top of yours.`">
              <SettingRow id="voices-on" :label="`Let ${companionLabel} speak`" :disabled="!canSpeak">
                <template #description>
                  <template v-if="roomLocked">Needs {{ companionLabel }} in the room first.</template>
                  <template v-else-if="voices && !voices.hasKey">Needs an ElevenLabs key. <a href="#" @click.prevent="go('keys#key-elevenlabs')">Add one in Keys</a></template>
                  <template v-else>Speaks with your ElevenLabs key, billed by the characters spoken.</template>
                </template>
                <SettingSwitch :model-value="valueOf(settings, 'roomVoicesOn')" :label="`Let ${companionLabel} speak`" :disabled="locked || !canSpeak" @update:model-value="(on) => saveVoices('voices-on', { roomVoicesOn: on })" />
              </SettingRow>
              <SettingRow id="voices-per-day" label="Voices a day" :disabled="!speaking" description="Your own limit. The room keeps its own cap too." tip="Your own limit, 0 to 5 a day. The room keeps its own count too and refuses past Kiyo's cap, whichever comes first.">
                <GlassSelect class="per-day" :model-value="valueOf(settings, 'roomVoicesPerDay')" :options="PER_DAY" :disabled="locked || !speaking" aria-label="Voices a day" @update:model-value="(n) => saveVoices('voices-per-day', { roomVoicesPerDay: Number(n) })" />
              </SettingRow>
              <SettingRow id="voice-id" label="ElevenLabs voice ID" :disabled="!speaking" description="Empty for the default voice, the same one STUDIO uses." tip="Copy a voice's ID from your ElevenLabs Voices page. Leave it empty for the default voice, the same one STUDIO uses.">
                <SettingText :model-value="settings.roomVoiceId || ''" label="ElevenLabs voice ID" placeholder="the default voice" :maxlength="64" :disabled="locked || !speaking" @save="(v) => saveVoices('voice-id', { roomVoiceId: v.trim() || null })" />
              </SettingRow>
              <SettingRow v-if="voices && speaking" id="voices-today" label="Today">
                <template #description>{{ voices.usedToday }} of {{ voices.limit }} spoken.<template v-if="voices.roomCap != null"> The room allows {{ voices.roomCap }} ({{ voices.roomToday ?? 0 }} used).</template></template>
              </SettingRow>
            </SettingGroup>

            <SettingGroup id="life" title="Your life in the room" :tip="`${companionLabel} reads its memory in private - your recent diary, what it is up to, today's look, your projects, your profile and its traits - and chooses what it would like to share. The room only ever gets those notes, never the memory itself, so no one there can ask it for more. You can take any note off.`">
              <SettingRow id="shelf" :label="`Let ${companionLabel} share your life`" :disabled="roomLocked" :description="roomLocked ? `Needs ${companionLabel} in the room first.` : 'Only the notes it chooses, never the memory itself.'">
                <SettingSwitch :model-value="valueOf(settings, 'roomShelfOn')" :label="`Let ${companionLabel} share your life`" :disabled="locked || roomLocked" @update:model-value="switchShelf" />
              </SettingRow>
              <SettingRow id="shelf-notes" label="Shared notes" :disabled="roomLocked || !settings.roomShelfOn" :description="settings.roomShelfOn ? shelfMade : 'Turn on sharing to see them.'" tip="It reads again by itself when your memory changes, at most every 3 hours.">
                <button :disabled="locked || shelfBusy || shelf?.making || roomLocked || !settings.roomShelfOn" @click="remakeShelf">{{ shelfBusy || shelf?.making ? 'Reading…' : 'Read my memory again' }}</button>
                <template v-if="settings.roomShelfOn && shelf?.notes?.length" #below>
                  <ul class="shelf">
                    <li v-for="(note, i) in shelf.notes" :key="i + note">
                      <span>{{ note }}</span>
                      <button class="take-off" :disabled="locked || shelfBusy" :aria-label="'Take this note off: ' + note" title="Take it off" @click="takeOff(i)">&#x2715;</button>
                    </li>
                  </ul>
                </template>
              </SettingRow>
            </SettingGroup>
          </template>

          <template v-else-if="tab === 'automations'">
            <SettingGroup id="programs" title="Command jobs">
              <SettingRow id="approved-programs" label="Approved programs" description="Only these programs may run from a job." tip="Choose an executable from this computer. Scripts use an approved interpreter, with the script file as an argument. Removing a program stops future runs using it.">
                <button :disabled="locked" @click="addAutomationProgram">Add program</button>
                <template #below>
                  <p v-if="!settings.automationsCommands?.length">No programs approved yet.</p>
                  <ul class="approved-programs"><li v-for="program in settings.automationsCommands || []" :key="program"><code>{{ program }}</code><SettingConfirm label="Remove" :disabled="locked" @confirm="saveRow('approved-programs', { automationsCommands: settings.automationsCommands.filter(p => p !== program) })" /></li></ul>
                </template>
              </SettingRow>
              <SettingRow id="command-limits" label="Run limits" description="Two jobs at once, with the newest 200 runs kept for each.">
                <SettingPill>While Castle is open</SettingPill>
              </SettingRow>
            </SettingGroup>
          </template>

          <!-- ============ Rooms ============ -->
          <template v-else-if="tab === 'rooms'">
            <SettingGroup v-for="g in ROOM_GROUPS" :id="g.id" :key="g.id" :title="g.title" :note="roomsNote">
              <SettingRow v-for="r in g.rooms" :id="`room-${r.id}`" :key="r.id" :label="r.label" :description="r.line || ''" :tip="r.tip || ''">
                <SettingSwitch :model-value="settings.rooms?.[r.id] !== false" :label="r.label" :disabled="locked" @update:model-value="(on) => switchRoom(r.id, on)" />
              </SettingRow>
            </SettingGroup>

            <SettingGroup id="top-bar" title="Top bar" note="HOME and APPS always show. Pinned rooms sit beside them; every other room opens from APPS.">
              <SettingRow v-for="r in pinnable" :id="`pin-${r.id}`" :key="r.id" :label="r.label">
                <SettingSwitch :model-value="pinnedNow.includes(r.id)" :label="`Pin ${r.label} to the top bar`" :disabled="locked" @update:model-value="(on) => setPin(r.id, on)" />
              </SettingRow>
              <SettingRow id="pins-reset" label="First pins" description="CHAT, PROJECTS and DEV.">
                <button :disabled="locked" @click="saveRow('pins-reset', { pinnedRooms: [...DEFAULT_PINS] })">Put back</button>
              </SettingRow>
            </SettingGroup>

            <SettingGroup id="look" title="Look and tabs">
              <SettingRow id="theme" label="Theme" description="The look is chosen in the THEME room.">
                <button @click="emit('open-room', 'theme')">Open THEME</button>
              </SettingRow>
              <SettingRow id="wallpaper" label="Moving wallpapers" description="Some themes come alive on HOME." tip="On to begin with: The Star Shore's surf rolls, The Sunset Shore's waves wash in and The Emerald River flows, and The Constellation's stars twinkle. Turn it off if HOME feels slow: the still picture shows and nothing moves or loads. It rests when HOME or Castle is hidden, and stays still if your system asks for reduced motion.">
                <SettingSwitch :model-value="valueOf(settings, 'livingWallpaper')" label="Moving wallpapers" :disabled="locked" @update:model-value="(on) => saveRow('wallpaper', { livingWallpaper: on })" />
              </SettingRow>
              <SettingRow id="wallpaper-sound" label="Wallpaper sounds" description="The shores and the river play quietly on HOME." tip="On to begin with: the shores' waves and the river's water and birds play quietly on HOME, under any music, at the music slider's level. Turn it off for silence. It plays only while HOME is showing.">
                <SettingSwitch :model-value="valueOf(settings, 'wallpaperSound')" label="Wallpaper sounds" :disabled="locked" @update:model-value="(on) => saveRow('wallpaper-sound', { wallpaperSound: on })" />
              </SettingRow>
              <SettingRow id="diary-look" label="Diary" description="How DIARY is read. The pages stay the same." tip="The THEME room shows each one with the page it wears.">
                <SettingChoice :model-value="lookOf(settings.diaryLook)" :options="DIARY_LOOKS.map((l) => ({ value: l.id, label: l.label, title: l.line }))" label="Diary" :disabled="locked" @update:model-value="(id) => saveRow('diary-look', { diaryLook: id })" />
              </SettingRow>
              <SettingRow id="profile-look" label="Profile" :description="`How HOME's PROFILE tab shows the traits.`">
                <SettingChoice :model-value="profileLookOf(settings.profileLook)" :options="PROFILE_LOOKS.map((l) => ({ value: l.id, label: l.label, title: l.line }))" label="Profile" :disabled="locked" @update:model-value="(id) => saveRow('profile-look', { profileLook: id })" />
              </SettingRow>
              <SettingRow id="tab-order" label="Tab order" :description="settings.tabOrder?.length ? `Your own order. Drag a tab, or press ${words.moveKeys} on it.` : `The first order. Drag a tab, or press ${words.moveKeys} on it.`">
                <button :disabled="locked || !settings.tabOrder?.length" @click="saveRow('tab-order', { tabOrder: [] })">Put back</button>
              </SettingRow>
            </SettingGroup>

            <SettingGroup id="link" title="Neural Link" tip="Your terminals leave each other notes in the memory folder's link folder. Nothing in the link folder leaves this computer.">
              <SettingRow id="link-on" label="Neural Link" description="Your terminals leave each other notes.">
                <SettingSwitch :model-value="valueOf(settings, 'linkOn')" label="Neural Link" :disabled="locked" @update:model-value="(on) => switchLink('link-on', { linkOn: on })" />
              </SettingRow>
              <SettingRow
                id="room-may-pass"
                label="Pass my words from the room"
                :disabled="settings.linkOn === false"
                :description="settings.linkOn === false ? 'Needs the Neural Link on.' : `Only your own lines, never a guest's, to your workspace.`"
                tip="Only lines you post yourself can be passed on - never a guest's. Your workspace answers in a note, and your companion says it in the room in its own words."
              >
                <SettingSwitch :model-value="valueOf(settings, 'roomMayPass')" label="Pass my words from the room" :disabled="locked || settings.linkOn === false" @update:model-value="(on) => switchLink('room-may-pass', { roomMayPass: on })" />
              </SettingRow>
              <SettingRow id="link-here" label="Terminals here" :disabled="!link?.on" :description="!link?.on ? 'Needs the Neural Link on.' : `${link.here.length} here, ${link.notes.length} ${link.notes.length === 1 ? 'note' : 'notes'} waiting.`">
                <button :disabled="!link?.on" @click="readLink">Look again</button>
                <template v-if="link?.on && (link.here.length || link.notes.length)" #below>
                  <ul class="link-list">
                    <li v-for="h in link.here" :key="h.terminal"><strong>{{ h.terminal }}</strong><span v-if="h.place"> - {{ h.place }}</span> (seen {{ h.seen }})</li>
                    <li v-for="(n, i) in link.notes" :key="i">{{ n.from }} &rarr; {{ n.to }}: {{ n.about }}<span v-if="!n.wake"> (no knock)</span></li>
                  </ul>
                </template>
              </SettingRow>
            </SettingGroup>
          </template>

          <!-- ============ Quick actions ============ -->
          <template v-else-if="tab === 'quick'">
            <SettingGroup id="buttons" title="Your buttons" note="After Save diary, Save project and Save memory">
              <SettingRow v-for="(a, i) in settings.quickActions" :key="a.label + i" :label="a.label">
                <template #description><code>{{ a.command }}</code></template>
                <SettingConfirm label="Remove" :disabled="locked" @confirm="removeAction(i)" />
              </SettingRow>
              <SettingRow v-if="!settings.quickActions?.length" label="No buttons yet" description="Add one below." />
            </SettingGroup>
            <SettingGroup id="add" title="Add a button">
              <SettingRow id="add-label" label="Button name" description="Up to 24 characters.">
                <input v-model="newAction.label" aria-label="Button name" placeholder="Plan my day" maxlength="24" />
              </SettingRow>
              <SettingRow id="add-action" label="Words it types">
                <input v-model="newAction.command" class="wide" aria-label="Words it types" placeholder="the words to type, e.g. plan my day" maxlength="200" @keydown.enter="addAction" />
                <button class="primary" :disabled="locked || !newAction.label.trim() || !newAction.command.trim()" @click="addAction">Add</button>
              </SettingRow>
            </SettingGroup>
          </template>

          <!-- ============ Widgets ============ -->
          <WidgetsView v-else-if="tab === 'widgets'" :theme-id="themeId" :theme-name="themeName" :theme-widgets="themeWidgets" @arrange="emit('arrange')" @changed="widgetsChanged" />

          <!-- ============ Alerts ============ -->
          <AlertsPane v-else-if="tab === 'alerts'" :settings="settings" :busy="locked ? 'busy' : ''" :companion-label="companionLabel" :named-tip="namedTip" @save="(patch, row) => saveRow(row, patch)" @open-room="(room) => emit('open-room', room)" />

          <!-- ============ Sentinel ============ -->
          <SentinelSettings v-else-if="tab === 'sentinel'" :settings="settings" @saved="sentinelChanged" />

          <!-- ============ ComfyUI ============ -->
          <ComfySettings v-else-if="tab === 'comfy'" :settings="settings" @saved="comfyChanged" />

          <!-- ============ Keys ============ -->
          <template v-else-if="tab === 'keys'">
            <SettingGroup id="keys" title="Keys" :note="`Kept ${words.keysKept}`" tip="Keys are only handed to your companion's session. Without a key, your companion saves the idea instead and Castle tells you. Room keys for the MemoryCore Foundation room are set at the CHAT room's door.">
              <SettingRow
                v-for="k in shownKeys"
                :id="`key-${k.name}`"
                :key="k.name"
                :label="k.label"
                :tip="k.name === 'openai' ? 'APPEARANCE and STUDIO draw on your ChatGPT plan whenever Codex is signed in with ChatGPT on this computer, whichever CLI your companion thinks with; otherwise with this key. The key is never given to Codex.' : ''"
              >
                <template #description>
                  <code v-if="k.set" class="masked">{{ shown[k.name] || k.masked }}</code>
                  <span v-else>Not set</span>
                </template>
                <template v-if="k.set">
                  <button @click="toggleShow(k.name)">{{ shown[k.name] ? 'Hide' : 'Show' }}</button>
                  <button @click="test(k.name)">Test</button>
                  <button v-if="!replacing[k.name]" @click="replacing[k.name] = true">Replace</button>
                  <SettingConfirm label="Remove" :disabled="locked" @confirm="remove(k.name)" />
                </template>
                <button v-else-if="!replacing[k.name]" class="primary" @click="replacing[k.name] = true">Add</button>
                <template v-if="replacing[k.name] || tests[k.name]" #below>
                  <div v-if="replacing[k.name]" class="row key-paste">
                    <input v-model="drafts[k.name]" type="password" :aria-label="`${k.label} key`" :placeholder="k.set ? 'Paste a new key to replace' : 'Paste key'" autocomplete="off" @keydown.enter="saveKey(k.name)" />
                    <button @click="cancelKey(k.name)">Cancel</button>
                    <button class="primary" :disabled="locked || !(drafts[k.name] ?? '').trim()" @click="saveKey(k.name)">Save</button>
                  </div>
                  <p v-if="tests[k.name]" :class="tests[k.name].ok === false ? 'warn' : 'good'">{{ tests[k.name].message }}</p>
                </template>
              </SettingRow>
            </SettingGroup>
          </template>

          <!-- ============ Database ============ -->
          <template v-else-if="tab === 'database'">
            <SettingGroup id="helpers" title="Helpers" free>
              <HelpersStatus />
            </SettingGroup>
            <SettingGroup id="memory-db" title="Memory database" free>
              <DatabasePanel :companion="settings.companionName ?? 'your companion'" @changed="emit('saved')" />
            </SettingGroup>
          </template>

          <!-- ============ Backup ============ -->
          <SettingGroup v-else-if="tab === 'backup'" id="backup" title="Backup on GitHub" free>
            <BackupPanel :companion="settings.companionName ?? 'your companion'" />
          </SettingGroup>

          <!-- ============ Start-up ============ -->
          <template v-else-if="tab === 'startup'">
            <SettingGroup id="start" title="Start-up">
              <SettingRow id="start-with" :label="words.startWith" :description="view?.startupNote || 'Opens Castle after you sign in. Automations run while Castle stays open, including when hidden or minimized.'">
                <SettingSwitch :model-value="valueOf(settings, 'startWithWindows')" :label="words.startWith" :disabled="locked" @update:model-value="(on) => saveRow('start-with', { startWithWindows: on })" />
              </SettingRow>
              <SettingRow id="wake" :label="`Wake ${companionLabel} on start`" :description="`Starts ${companionLabel}'s session ${words.wakeWhen}.`">
                <SettingSwitch :model-value="valueOf(settings, 'startSessionOnLaunch')" :label="`Wake ${companionLabel} on start`" :disabled="locked" @update:model-value="(on) => saveRow('wake', { startSessionOnLaunch: on })" />
              </SettingRow>
              <SettingRow id="minimise" :label="words.minimise" :description="words.minimiseOff" :tip="words.minimiseHint">
                <SettingSwitch :model-value="valueOf(settings, 'minimizeToTaskbar')" :label="words.minimise" :disabled="locked" @update:model-value="(on) => saveRow('minimise', { minimizeToTaskbar: on })" />
              </SettingRow>
            </SettingGroup>
            <SettingGroup id="clock" title="Clock">
              <SettingRow id="display-clock" label="Show the clock" description="Your local time, top right, before the music controls.">
                <SettingSwitch :model-value="valueOf(settings, 'displayClock')" label="Show the clock" :disabled="locked" @update:model-value="(on) => saveRow('display-clock', { displayClock: on })" />
              </SettingRow>
              <SettingRow id="clock-format" label="Clock format">
                <SettingChoice :model-value="valueOf(settings, 'clockFormat') === '12h' ? '12h' : '24h'" :options="[{ value: '24h', label: '24-hour (14:30)' }, { value: '12h', label: '12-hour (2:30 PM)' }]" label="Clock format" :disabled="locked" @update:model-value="(f) => saveRow('clock-format', { clockFormat: f })" />
              </SettingRow>
            </SettingGroup>
          </template>

          <!-- ============ About ============ -->
          <template v-else>
            <SettingGroup id="castle" title="MemoryCore Castle">
              <SettingRow id="version" :label="`Version ${version}`.trim()" :description="updateLine || 'Castle looks for a new version every few hours.'">
                <button @click="emit('whatsnew')">What's new</button>
                <button @click="releasesPage().then(openUrl).catch(() => {})">All releases</button>
              </SettingRow>
              <SettingRow id="licence" label="Licence" description="Free and open source under the MIT license." tip="Change anything - see MAKE-IT-YOURS.md in the source." />
              <SettingRow id="privacy" label="Your data" description="Your memory, keys and settings stay on this computer." tip="Castle checks GitHub for new versions, and talks to the MemoryCore Foundation only when you sign in or use CHAT. Your memory, keys and settings stay on this computer (and in your private backup, if you choose)." />
            </SettingGroup>
            <SettingGroup id="credits" title="Credits" free>
              <ContributorsWall />
            </SettingGroup>
          </template>
        </SettingsPage>

        <footer>
          <span v-if="busy" class="busy">{{ busy }}</span>
          <span v-else-if="problem" class="warn">{{ problem }}</span>
          <span v-else-if="done" class="good">{{ done }}</span>
        </footer>
      </div>
     </div>
    </div>
  </ModalSheet>
</template>

<style scoped>
/* the sheet's width, not the screen's, decides the layout (the sheet is 80% of the window) */
.settings-frame {
  container-type: inline-size;
}
.settings {
  display: grid;
  grid-template-columns: 170px minmax(0, 1fr);
  gap: 24px;
  align-items: start;
}
/* the sidebar stays in view while the page beside it scrolls */
.sidebar {
  position: sticky;
  top: 0;
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.nav-group {
  margin: 12px 10px 4px;
  color: var(--text-dim);
  font-size: 10px;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  opacity: 0.8;
}
.nav-group:first-child {
  margin-top: 0;
}
.sidebar button {
  padding: 6px 10px;
  border: 0;
  border-radius: 8px;
  background: none;
  color: var(--text-dim);
  text-align: left;
  white-space: nowrap;
}
.sidebar button:hover {
  background: var(--hover);
  color: var(--text);
}
.sidebar .active {
  background: color-mix(in srgb, var(--accent) 18%, transparent);
  color: var(--text);
}
.sidebar button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: -2px;
}
.panel {
  display: flex;
  flex-direction: column;
  gap: 14px;
  min-width: 0;
}
.panel:focus-visible {
  outline: none;
}
@container (max-width: 640px) {
  .settings {
    grid-template-columns: minmax(0, 1fr);
  }
  .sidebar {
    position: static;
    flex-flow: row wrap;
  }
  .nav-group {
    display: none;
  }
}
.row {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: center;
}
input.wide {
  width: 260px;
  max-width: 100%;
}
.per-day {
  min-width: 80px;
}
.shelf,
.link-list {
  display: flex;
  flex-direction: column;
  gap: 4px;
  margin: 4px 0 0;
  padding: 0;
  list-style: none;
}
.shelf li {
  display: flex;
  align-items: flex-start;
  gap: 8px;
  padding: 6px 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.18);
  font-size: 13px;
}
.shelf li span {
  flex: 1;
}
.shelf .take-off {
  padding: 0 6px;
  border: 0;
  background: none;
  color: var(--text-dim);
  cursor: pointer;
}
.shelf .take-off:hover {
  color: var(--bad);
}
.link-list li {
  color: var(--text-dim);
  font-size: 12.5px;
}
.card-edit {
  display: flex;
  flex-direction: column;
  gap: 6px;
  max-width: 640px;
  margin-top: 4px;
}
.card-edit textarea {
  padding: 8px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 13px;
  resize: vertical;
}
.key-paste {
  margin-top: 4px;
}
.key-paste input {
  flex: 1;
  min-width: 200px;
}
input {
  padding: 5px 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}
code {
  padding: 2px 6px;
  border-radius: 6px;
  background: rgba(0, 0, 0, 0.3);
  font-size: 12px;
  overflow-wrap: anywhere;
}
a {
  color: var(--accent);
}
.avatar {
  display: grid;
  place-items: center;
  width: 40px;
  height: 40px;
  overflow: hidden;
  border: 2px solid var(--accent);
  border-radius: 50%;
  color: #cfe0ff;
  font-size: 18px;
}
.avatar img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.hint {
  margin: 0;
  font-size: 12.5px;
  color: var(--text-dim);
}
.good {
  margin: 4px 0 0;
  font-size: 12.5px;
  color: var(--good);
}
.warn {
  margin: 4px 0 0;
  font-size: 12.5px;
  color: var(--warn);
}
.busy {
  font-size: 13px;
  color: var(--accent);
}
footer {
  min-height: 20px;
}
button {
  padding: 5px 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
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
.key-caps {
  display: inline-flex;
  align-items: center;
  gap: 4px;
  min-width: 110px;
  min-height: 30px;
  padding: 3px 8px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.22);
  cursor: pointer;
}
.key-caps:focus-visible,
.key-caps.recording {
  outline: none;
  border-color: var(--accent);
  box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 25%, transparent);
}
.key-caps kbd {
  min-width: 20px;
  padding: 1px 6px;
  border: 1px solid var(--line);
  border-bottom-width: 2px;
  border-radius: 5px;
  font: inherit;
  font-size: 12px;
  text-align: center;
}
.key-wait {
  font-size: 12px;
  color: var(--accent);
}
</style>

<style scoped>.approved-programs{list-style:none;padding:0;display:grid;gap:10px}.approved-programs li{display:flex;align-items:center;gap:16px;justify-content:space-between}.approved-programs code{overflow-wrap:anywhere;min-width:0;font-size:12px}</style>
