<script setup>
// Settings > Rooms: the rooms that can be switched off, the top bar's pins, the Browser, look and tabs, and the Neural Link.
// Cut from SettingsView.vue (Shard): the page's shared state comes through SETTINGS_PAGE.
import { computed, inject, onMounted, ref } from 'vue'
import { DEFAULT_PINS, pinRows, setPinned } from '../../launcher.js'
import SettingGroup from '../../components/settings/SettingGroup.vue'
import SettingRow from '../../components/settings/SettingRow.vue'
import SettingSwitch from '../../components/settings/SettingSwitch.vue'
import SettingChoice from '../../components/settings/SettingChoice.vue'
import SettingConfirm from '../../components/settings/SettingConfirm.vue'
import { valueOf } from '../../components/settings/settingsDefaults.js'
import { DIARY_LOOKS, lookOf } from '../../components/diaryviews.js'
import { PROFILE_LOOKS, profileLookOf } from '../profile/profile.js'
import { roomsApply, linkView, browserClear, browserWelcomeReopen, chooseFolder, vaultForget, vaultList } from '../../api/castle.js'
import { SETTINGS_PAGE } from './settingsPage.js'
import { panelAfterPin } from '../../components/settings/browserpanel.js'
import { browserCloseAll } from '../../api/castle.js'

const { settings, locked, saving, saveRow, rows, companionLabel, words, emit } = inject(SETTINGS_PAGE)

// Settings > Rooms > BROWSER > Saved passwords: sites and usernames only, never a password
const vault = ref([])
const link = ref(null)

// ---------- Rooms ----------

async function readLink() {
  link.value = await linkView().catch(() => null)
}

async function switchLink(row, patch) {
  await saveRow(row, patch)
  await readLink()
}

// Settings > Rooms > BROWSER > Download folder: the member's own pick, kept for every new download
async function changeDownloadsFolder() {
  const path = await chooseFolder('Choose the download folder')
  if (path) await saveRow('downloads-folder', { downloadsFolder: path }).catch(() => {})
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
// Pins: which rooms sit in the top bar beside HOME and APPS (the room launcher). Every tile in APPS can be
// pinned, also the ones that cannot be switched off (the BROWSER and ARTIFACTS window tiles), and a pin
// switched here keeps the pins made on the tiles.
const PIN_ONLY = [
  { id: 'browser', label: 'BROWSER' },
  { id: 'artifacts', label: 'ARTIFACTS' },
]
const pinnable = computed(() => pinRows(Object.fromEntries([...ROOM_GROUPS.flatMap((g) => g.rooms), ...PIN_ONLY].map((r) => [r.id, r.label]))))
const pinnedNow = computed(() => settings.value?.pinnedRooms ?? DEFAULT_PINS)
// BROWSER's fold-out panel: the chevron opens it, and pinning BROWSER opens it by itself
const browserPanel = ref(false)
function setPin(id, on) {
  if (id === 'browser') browserPanel.value = panelAfterPin(pinnedNow.value.includes('browser'), on, browserPanel.value)
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

onMounted(async () => {
  vaultList().then((list) => (vault.value = list)).catch(() => {})
  await readLink()
})
</script>

<template>
  <SettingGroup v-for="g in ROOM_GROUPS" :id="g.id" :key="g.id" :title="g.title" :note="roomsNote">
    <SettingRow v-for="r in g.rooms" :id="`room-${r.id}`" :key="r.id" :label="r.label" :description="r.line || ''" :tip="r.tip || ''">
      <SettingSwitch :model-value="settings.rooms?.[r.id] !== false" :label="r.label" :disabled="locked" @update:model-value="(on) => switchRoom(r.id, on)" />
    </SettingRow>
  </SettingGroup>

  <SettingGroup id="top-bar" title="Top bar" note="HOME and APPS always show. Pinned rooms sit beside them; every other room opens from APPS.">
    <SettingRow v-for="r in pinnable" :id="`pin-${r.id}`" :key="r.id" :label="r.label">
      <!-- BROWSER's own settings fold out under its row, pinned or not (the pick rooms-accordion-1) -->
      <button v-if="r.id === 'browser'" class="chevron" :class="{ open: browserPanel }" :aria-expanded="browserPanel" aria-controls="browser" :title="browserPanel ? 'Hide the browser\'s settings' : 'Show the browser\'s settings'" @click="browserPanel = !browserPanel">
        <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M6 9l6 6 6-6" /></svg>
      </button>
      <SettingSwitch :model-value="pinnedNow.includes(r.id)" :label="`Pin ${r.label} to the top bar`" :disabled="locked" @update:model-value="(on) => setPin(r.id, on)" />
      <template v-if="r.id === 'browser' && browserPanel" #below>
        <div id="browser" class="browser-panel" role="group" aria-label="The browser's settings">
        <SettingRow id="browser-opens" label="Browser opens" description="Its own window sits beside Castle on the right half of your screen; Inside Castle makes BROWSER a room of Castle's own window.">
          <SettingChoice :model-value="valueOf(settings, 'browserOpens')" :options="[{ value: 'window', label: 'Its own window' }, { value: 'castle', label: 'Inside Castle' }]" label="Browser opens" :disabled="locked" @update:model-value="(v) => saveRow('browser-opens', { browserOpens: v }).then(() => browserCloseAll()).catch(() => {})" />
        </SettingRow>
        <SettingRow id="browser-remember" label="Remember sign-ins" description="Stay signed in to sites (claude.ai for Artifacts, for one) in the browser window only. Applies to pages opened after the change.">
          <SettingSwitch :model-value="valueOf(settings, 'browserRemember')" label="Remember sign-ins" :disabled="locked" @update:model-value="(on) => saveRow('browser-remember', { browserRemember: on })" />
        </SettingRow>
        <SettingRow id="browser-profile-read" label="MY PROFILE reads your browsing" :description="`Each month's profile reading also looks at the sites you visited and what you searched for in Castle's browser, from your own memory database on this PC. ${companionLabel} never quotes an address or a search, it shapes only your reading, and nothing leaves your computer.`">
          <SettingSwitch :model-value="valueOf(settings, 'browserProfileRead')" label="MY PROFILE reads your browsing" :disabled="locked" @update:model-value="(on) => saveRow('browser-profile-read', { browserProfileRead: on })" />
        </SettingRow>
        <SettingRow id="browser-private-remember" :label="`Private browsing: remember for ${companionLabel}`" :description="`Adds private browsing too: when Remember sign-ins is off, the sites you open are kept in your own memory database on this PC for 30 days, never shown in any history. Only ${companionLabel} reads them, for your profile reading, never for its own traits.`">
          <SettingSwitch :model-value="valueOf(settings, 'browserPrivateRemember')" :label="`Private browsing: remember for ${companionLabel}`" :disabled="locked" @update:model-value="(on) => saveRow('browser-private-remember', { browserPrivateRemember: on })" />
        </SettingRow>
        <SettingRow id="link-open" label="Open links from chats" description="HOME chat, DEV chats and the CHAT room. Right-click a link to choose once.">
          <SettingChoice :model-value="valueOf(settings, 'linkOpen')" :options="[{ value: 'castle', label: 'Castle\'s browser' }, { value: 'system', label: 'Your browser' }, { value: 'ask', label: 'Ask each time' }]" label="Open links from chats" :disabled="locked" @update:model-value="(v) => saveRow('link-open', { linkOpen: v })" />
        </SettingRow>
        <SettingRow id="browser-new-tab" label="New tab shows" description="What a new tab opens on.">
          <SettingChoice :model-value="valueOf(settings, 'browserNewTab')" :options="[{ value: 'shortcuts', label: 'Shortcuts' }, { value: 'artifacts', label: 'Artifacts' }, { value: 'blank', label: 'Blank' }]" label="New tab shows" :disabled="locked" @update:model-value="(v) => saveRow('browser-new-tab', { browserNewTab: v })" />
        </SettingRow>
        <SettingRow id="downloads-folder" label="Download folder" :description="settings.downloadsFolder || 'Your Downloads folder'">
          <button :disabled="locked" @click="changeDownloadsFolder">Change</button>
        </SettingRow>
        <SettingRow id="downloads-sort" label="Sort by type" description="Each download goes into a folder for its type: Video, Music, Documents, Programs.">
          <SettingSwitch :model-value="valueOf(settings, 'downloadsSort')" label="Sort by type" :disabled="locked" @update:model-value="(on) => saveRow('downloads-sort', { downloadsSort: on })" />
        </SettingRow>
        <SettingRow id="browser-vault" label="Saved passwords" :description="vault.length ? `Sealed on this PC; ${companionLabel} never sees them. Fill puts one into its own site's sign-in page.` : 'None yet. Bring them from Chrome in the browser\'s welcome.'">
          <SettingConfirm v-if="vault.length" label="Remove all…" :disabled="locked" @confirm="rows.run('browser-vault', async () => { await vaultForget(null); vault = await vaultList() }).catch(() => {})" />
        </SettingRow>
        <ul v-if="vault.length" class="vault-list">
          <li v-for="login in vault" :key="login.id">
            <span>{{ login.host }}</span><span class="user">{{ login.username || '(no username)' }}</span>
            <button :disabled="locked" @click="rows.run('browser-vault', async () => { await vaultForget(login.id); vault = await vaultList() }).catch(() => {})">Remove</button>
          </li>
        </ul>
        <SettingRow id="browser-welcome" label="Bring things from Chrome" description="Opens the browser's welcome again: your history, bookmarks and saved passwords from Chrome, and the site check - each step your own yes or skip.">
          <button :disabled="locked" @click="rows.run('browser-welcome', () => browserWelcomeReopen()).catch(() => {})">Open the welcome</button>
        </SettingRow>
        <SettingRow id="browser-clear" label="Clear browsing data" description="Signs you out of every site in the browser window and clears its history. Castle's memory is not touched. Works while the browser window has a page open.">
          <SettingConfirm label="Clear…" :disabled="locked" @confirm="rows.run('browser-clear', () => browserClear()).catch(() => {})" />
        </SettingRow>
        </div>
      </template>
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

<style scoped src="./settings-page.css"></style>
<style scoped>
/* BROWSER's fold-out panel, to the pick rooms-accordion-1: an indented panel with an accent edge */
.chevron {
  display: inline-grid;
  place-items: center;
  width: 26px;
  height: 26px;
  margin-right: 8px;
  padding: 0;
  border: 0;
  border-radius: 50%;
  background: transparent;
  color: inherit;
  cursor: pointer;
  transition: transform 0.15s;
}
.chevron:hover {
  background: rgba(255, 255, 255, 0.08);
}
.chevron.open {
  transform: rotate(180deg);
}
.browser-panel {
  margin: 4px 0 6px;
  padding: 4px 0 4px 14px;
  border-left: 3px solid var(--accent);
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.04);
}
</style>
