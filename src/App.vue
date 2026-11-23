<script setup>
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import TopBar from './components/TopBar.vue'
import TerminalPanel from './components/TerminalPanel.vue'
import ClockWidget from './components/widgets/ClockWidget.vue'
import GaugeWidget from './components/widgets/GaugeWidget.vue'
import WeatherWidget from './components/widgets/WeatherWidget.vue'
import DisksWidget from './components/widgets/DisksWidget.vue'
import SentinelWidget from './components/widgets/SentinelWidget.vue'
import SweepWidget from './components/widgets/SweepWidget.vue'
import AutomationsWidget from './components/widgets/AutomationsWidget.vue'
import CalendarWidget from './components/widgets/CalendarWidget.vue'
import SetupView from './views/SetupView.vue'
import HomeCards from './components/widgets/HomeCards.vue'
import UsageWidget from './components/widgets/UsageWidget.vue'
import CreditWidget from './components/widgets/CreditWidget.vue'
import DatabaseOffer from './components/DatabaseOffer.vue'
import { shouldOffer } from './components/database.js'
import CompanionHeader from './components/CompanionHeader.vue'
import { alertActiveRoom, nodePrepare, onNodeInstall, onOpenChat, onOpenRoom, onProviderChanged } from './api/castle.js'
import { prepareStudio } from './composables/useStudio.js'
import { prepareLibrary } from './composables/useLibrary.js'
import { loadUserThemes } from './composables/useThemes.js'
import { prepareAppearance } from './composables/useAppearance.js'
import { prepareWorld } from './composables/useWorld.js'
import { loaderBegin, loaderDone, loaderEnd, loaderExpect, loaderRemember } from './loader/loader.js'
import ModalSheet from './components/ModalSheet.vue'
import BackupPanel from './components/BackupPanel.vue'
import SentinelSheet from './components/SentinelSheet.vue'
import AppsView from './views/AppsView.vue'
import MyProfileView from './views/myprofile/MyProfileView.vue'
import MemoryRoom from './views/MemoryRoom.vue'
import LibraryView from './views/LibraryView.vue'
import MarketView from './views/MarketView.vue'
import MessagingView from './views/MessagingView.vue'
import AutomationsView from './views/AutomationsView.vue'
import CalendarView from './views/CalendarView.vue'
import LauncherView from './views/LauncherView.vue'
import { DEFAULT_PINS, badgesFor, cleanPins, headerTabs, opensLauncher, setPinned } from './launcher.js'
import { calendarCard, prepareCalendar, refreshCalendarCard } from './composables/useCalendar.js'
import { automations, prepareAutomations } from './composables/useAutomations.js'
import ThemeView from './views/ThemeView.vue'
import StudioView from './views/StudioView.vue'
import AppearanceView from './views/AppearanceView.vue'
import StoryView from './views/StoryView.vue'
import WorldView from './views/WorldView.vue'
import ChatRoomView from './views/ChatRoomView.vue'
import DevView from './views/DevView.vue'
import SettingsView from './views/SettingsView.vue'
import UpdateChip from './components/UpdateChip.vue'
import QuotaGlow from './components/QuotaGlow.vue'
import UsageReveal from './components/UsageReveal.vue'
import ThemeMotion from './components/ThemeMotion.vue'
import ThemeVideo from './components/ThemeVideo.vue'
import ThemeSound from './components/ThemeSound.vue'
import WhatsNew from './components/WhatsNew.vue'
import { sectionFor, unseenSections } from './components/changelog.js'
import { moveTab, orderTabs } from './components/taborder.js'
import { WIDGETS, dragBegins, dropPlace, isOn, memberPlace, originOf, placeFor, placePatch, resetPatch, adoptPatch, unplaced, scaleOf, startsOnControl, hasItsKey, roomIsOn } from './components/widgets/widgetplaces.js'
import { isCreditWidget } from './components/widgets/credits.js'
import changelog from '../CHANGELOG.md?raw'
import { version as appVersion } from '../package.json'
import NoticeToasts from './components/NoticeToasts.vue'
import MeterAlert from './components/MeterAlert.vue'
import AmbientMusic from './components/AmbientMusic.vue'
import { avatarGet, backupNow, backupStatus, dbMemory, dbStart, dbStatus, getSettings, onAutomations, onCalendar, hideCastle, telegramBridgeStatus, discordBridgeStatus, whatsappBridgeStatus, keyStatus, memoryHome, saveSettings, onBackupDone, onStoryOutput, versionSeen } from './api/castle.js'
import { DEFAULT_THEME_ID, applyTheme, builtInThemes, chooseTheme, fromUserFolders, placeStyle, skinClass } from './themes/loader.js'

// The rooms across the top. A room whose MemoryCore feature is not installed stays hidden
// (`needs`); the rest always show.
const allTabs = [
  { id: 'home', label: 'HOME' },
  { id: 'theme', label: 'THEME' },
  // Kiyo's room of Start-menu apps. Its id stays `apps` (saved tab order, room switches and the code all use it);
  // only the label is DESKTOP, so the name APPS can belong to the room launcher (id `launcher`) that comes next.
  { id: 'apps', label: 'DESKTOP' },
  { id: 'myprofile', label: 'MY PROFILE' },
  { id: 'diary', label: 'DIARY', needs: 'diary' },
  { id: 'projects', label: 'PROJECTS', needs: 'projects' },
  { id: 'library', label: 'LIBRARY' },
  { id: 'appearance', label: 'APPEARANCE' },
  { id: 'studio', label: 'STUDIO' },
  { id: 'story', label: 'STORY' },
  { id: 'world', label: 'WORLD' },
  { id: 'chat', label: 'CHAT' },
  { id: 'dev', label: 'DEV' },
  { id: 'market', label: 'MARKET' },
  { id: 'messaging', label: 'MESSAGING' },
  { id: 'automations', label: 'AUTOMATIONS' },
  { id: 'calendar', label: 'CALENDAR' },
]
// dev only: ?tab=<room> opens a room straight away (for checking a room in a browser)
const devTab = import.meta.env.DEV ? new URLSearchParams(location.search).get('tab') : null
const active = ref(devTab || 'home')
// What HOME shows (memory_home), the backup card, the portrait and the CLI chip.
const home = ref(null)
let stopAutomations
let stopCalendar
const backup = ref(null)
// the DATABASE card: { engine, memory }
const database = ref(null)
// the one-time offer after an update (memory still in files, no database yet)
const databaseOffer = ref(false)
const databaseBytes = ref(0)
const databaseSource = ref('download')
const offerFacts = ref({ offerSeen: true, hadCastleBefore: false, hasFolder: false })
const avatar = ref(null)
const memoryFolder = ref('')
const backupOpen = ref(false)
// the Sentinel, opened from its shield on the top bar
const sentinelOpen = ref(false)
// the Settings sheet and the section it opens on
const settingsOpen = ref(false)
const settingsSection = ref('companion')
// the update line for Settings > About
const updateInfo = ref(null)
// Rooms that are built; any other room shows its name until its phase lands.
const BUILT = new Set(['launcher', 'home', 'theme', 'apps', 'myprofile', 'diary', 'projects', 'library', 'studio', 'appearance', 'story', 'world', 'chat', 'dev', 'market', 'messaging', 'automations', 'calendar'])
// Tabs with something new waiting (the story speaking while the member is elsewhere).
const marks = ref([])
const memberName = ref('')
const storyOpened = ref(active.value === 'story')
const chatOpened = ref(active.value === 'chat')
// DEV stays mounted once opened, so its project sessions keep going behind the other rooms
const devOpened = ref(active.value === 'dev')
// where APPS was opened from, so Esc there goes back
const previousRoom = ref('home')
const launcherView = ref(null)
const launcherSearch = ref(false)
watch(active, (room, was) => {
  if (room !== 'launcher') launcherSearch.value = false
  if (room === 'launcher' && was && was !== 'launcher') previousRoom.value = was
  if (room === 'launcher') refreshPending()
  alertActiveRoom(room).catch(() => {})
  if (room !== 'home') arranging.value = false
  if (room === 'story') storyOpened.value = true
  if (room === 'chat') chatOpened.value = true
  if (room === 'dev') devOpened.value = true
})
// a file a HOME card asked to open in its room
const roomFile = ref('')
const memoryStamp = ref(0)
// rooms the member switched off in Settings are not available (never removed)
const rooms = ref({})
const barClock = ref({ enabled: false, format: '24h' })
const usageCard = ref(true)
// HOME's widgets as the member arranged them (WIDGETS): on or off, size, and places per theme
const widgetPrefs = ref({})
const arranging = ref(false)
const drag = ref(null) // { id, x, y, dx, dy, box }
const homeEl = ref(null)
// the member's own tab order (dragged in the top bar); empty = Castle's order
const tabOrder = ref([])
const tabs = computed(() =>
  // APPS shows EVERY room; the per-room switches in Settings no longer hide a room here or in the top bar (the member pins what they use). Only a room whose MemoryCore feature is missing stays out.
  orderTabs(allTabs, tabOrder.value).filter((tab) => !tab.needs || home.value?.features?.[tab.needs]),
)
// Chats waiting for the member's approval (Telegram, Discord, WhatsApp): the badge on MESSAGING.
const pendingChats = ref(0)
async function refreshPending() {
  if (rooms.value.messaging === false || !rooms.value.messaging) { pendingChats.value = 0; return }
  const got = await Promise.allSettled([telegramBridgeStatus(), discordBridgeStatus(), whatsappBridgeStatus()])
  pendingChats.value = got.filter((g) => g.status === 'fulfilled' && g.value?.pending).length
}
// What each tile in APPS says is waiting: only state Castle already holds, nothing is fetched for it.
const badges = computed(() => badgesFor({
  automations: rooms.value.automations ? automations.value : null,
  pending: pendingChats.value,
  marks: marks.value,
  calendar: rooms.value.calendar ? calendarCard.value : null,
}))
// The rooms pinned to the top bar (Settings `pinnedRooms`); every other room opens from APPS, the room launcher.
const pins = ref([...DEFAULT_PINS])
// What the top bar holds: HOME, APPS, the pinned rooms, and the open room if it is not pinned.
const barTabs = computed(() => headerTabs(tabs.value, pins.value, active.value))
// A room that is not in the bar can still have something new (the story speaking): APPS says so.
const barMarks = computed(() => {
  const shown = new Set(barTabs.value.map((t) => t.id))
  return marks.value.some((m) => !shown.has(m)) ? [...marks.value, 'launcher'] : marks.value
})
// a room switched off closes: its tab goes, its mark clears, a kept-alive room unmounts
watch(tabs, (list) => {
  const ids = new Set(list.map((t) => t.id))
  if (!ids.has(active.value) && active.value !== 'launcher') active.value = 'home'
  if (!ids.has('story')) storyOpened.value = false
  if (!ids.has('chat')) chatOpened.value = false
  if (!ids.has('dev')) devOpened.value = false
  marks.value = marks.value.filter((m) => ids.has(m))
})
const companion = ref('Your companion')
const cli = ref('claude')
const settingsStamp = ref(0)
// The first-run setup: open until it is done; reopened from the panel or the gear.
const setupOpen = ref(false)
const setupReopened = ref(false)
const theme = ref(null)
// Settings > Rooms > Moving wallpapers (livingWallpaper): a theme's looping video (ThemeVideo) or drawn
// motion (ThemeMotion), on unless the member turned it off; off, HOME shows the still picture
const wallpaperMotion = ref(false)
// Settings > Rooms > Wallpaper sounds (wallpaperSound): a theme's looping sound (ThemeSound), on unless turned off
const wallpaperSound = ref(false)
const themeNotice = ref('')
const activeLabel = computed(() => allTabs.find((t) => t.id === active.value)?.label ?? '')
const widgets = computed(() => theme.value?.widgets ?? {})
// The keys saved in Settings > Keys: a credit widget shows only once its key is there.
const savedKeys = ref([])
// The widgets HOME draws: switched on, placed by the theme (all but the clock and the gauges),
// and for a credit widget, its key saved.
// clock, gauges, Sweep and weather show even when a theme names no place for them (Sweep and weather then take their own)
const SELF_PLACED = ['clock', 'gauges', 'sweep', 'weather', 'disks', 'sentinel', 'automations', 'calendar']
const FALLBACK_PLACES = { sweep: { right: '31%', top: '22%' }, weather: { left: '2%', bottom: '4%' }, disks: { right: '2%', bottom: '4%' }, sentinel: { right: '43%', top: '22%' }, automations: { right: '2%', top: '38%' }, calendar: { right: '2%', top: '62%' } }
const shownWidgets = computed(() =>
  WIDGETS.map((w) => w.id).filter(
    (id) => isOn(id, { ...widgetPrefs.value, showUsageCard: usageCard.value }) && (SELF_PLACED.includes(id) || widgets.value[id] || memberPlace(widgetPrefs.value, theme.value?.id, id)) && hasItsKey(id, savedKeys.value) && roomIsOn(id, rooms.value),
  ),
)

function slotStyle(id) {
  const place = placeFor(widgets.value[id] ?? FALLBACK_PLACES[id], memberPlace(widgetPrefs.value, theme.value?.id, id))
  const style = placeStyle(place)
  // the member's size times the theme's starting scale, grown from the corner it is anchored to
  const scale = scaleOf(id, widgetPrefs.value, theme.value)
  const moving = drag.value?.id === id ? `translate(${drag.value.dx}px, ${drag.value.dy}px) ` : ''
  if (moving || scale !== 1) style.transform = `${moving}scale(${scale})`
  if (scale !== 1) style.transformOrigin = originOf(place)
  return style
}

// ONE arrangement for every theme: a widget that has no kept place yet (the first start, a new widget
// after an update, a theme's first look) is kept where it stands now, once its card has settled, so a
// theme change or a restart never moves it. The theme's own places only decide that first spot.
let adoptTimer = null
function keepPlacesSoon() {
  clearTimeout(adoptTimer)
  adoptTimer = setTimeout(keepPlaces, 2500)
}
async function keepPlaces() {
  const el = homeEl.value
  if (!el || active.value !== 'home' || arranging.value || drag.value || !theme.value) return
  const home = el.getBoundingClientRect()
  if (!home.width || !home.height) return
  const found = {}
  for (const id of unplaced(widgetPrefs.value, theme.value.id, shownWidgets.value)) {
    const slot = el.querySelector(`[data-widget="${id}"]`)
    if (!slot) continue
    const r = slot.getBoundingClientRect()
    if (!r.width || !r.height) continue
    found[id] = dropPlace({ left: r.left - home.left, top: r.top - home.top, width: r.width, height: r.height }, { width: home.width, height: home.height })
  }
  const patch = adoptPatch(widgetPrefs.value, theme.value.id, found)
  if (patch) await saveWidgets(patch)
}
watch([shownWidgets, theme, active, () => widgetPrefs.value.widgetPlaces], keepPlacesSoon)

async function saveWidgets(patch) {
  widgetPrefs.value = { ...widgetPrefs.value, ...patch }
  try {
    const { settings } = await getSettings()
    await saveSettings({ ...settings, ...patch })
  } catch {
    // outside Castle's window: the arrangement holds until the page reloads
  }
}

// A widget follows the pointer and is put down where it is dropped, kept on HOME - in arrange mode
// at once, and on HOME itself once the pointer has moved a few pixels (a still press stays a click,
// and a press on a button, link or field never drags).
let pressed = null // { id, x, y, box, pointerId, el } until the drag begins
function startDrag(event, id) {
  if (event.button !== 0) return
  const tags = event.composedPath().filter((n) => n instanceof Element && event.currentTarget.contains(n)).map((n) => n.tagName)
  if (!arranging.value && startsOnControl(tags)) return
  const home = homeEl.value.getBoundingClientRect()
  const rect = event.currentTarget.getBoundingClientRect()
  pressed = { id, x: event.clientX, y: event.clientY, pointerId: event.pointerId, el: event.currentTarget, box: { left: rect.left - home.left, top: rect.top - home.top, width: rect.width, height: rect.height } }
  if (arranging.value) beginDrag()
}

function beginDrag() {
  pressed.el.setPointerCapture(pressed.pointerId)
  drag.value = { id: pressed.id, x: pressed.x, y: pressed.y, dx: 0, dy: 0, box: pressed.box }
}

function moveDrag(event) {
  if (pressed && !drag.value && dragBegins(event.clientX - pressed.x, event.clientY - pressed.y)) beginDrag()
  if (!drag.value) return
  drag.value = { ...drag.value, dx: event.clientX - drag.value.x, dy: event.clientY - drag.value.y }
}

async function endDrag() {
  pressed = null
  const d = drag.value
  if (!d) return
  drag.value = null
  if (!d.dx && !d.dy) return
  // the release after a drag is not a click on whatever card row it ended over
  const swallow = (e) => { e.stopPropagation(); e.preventDefault() }
  window.addEventListener('click', swallow, { capture: true, once: true })
  setTimeout(() => window.removeEventListener('click', swallow, { capture: true }), 0)
  const home = homeEl.value.getBoundingClientRect()
  const place = dropPlace({ ...d.box, left: d.box.left + d.dx, top: d.box.top + d.dy }, { width: home.width, height: home.height })
  await saveWidgets(placePatch(widgetPrefs.value, theme.value?.id, d.id, place))
}

function arrange() {
  active.value = 'home'
  arranging.value = true
}

const onArrangeKey = (event) => event.key === 'Escape' && arranging.value && (arranging.value = false)

// "What's new" after an update: the sections since the version last seen, once. The very first
// launch only records the version. dev only: ?whatsnew=<version> shows that version's notes.
const whatsNew = ref(null)
let whatsNewChecked = false
async function checkWhatsNew(lastSeen) {
  if (whatsNewChecked) return
  whatsNewChecked = true
  const devWhatsNew = import.meta.env.DEV ? new URLSearchParams(location.search).get('whatsnew') : null
  if (devWhatsNew) {
    const s = sectionFor(changelog, devWhatsNew)
    whatsNew.value = { version: devWhatsNew, sections: s ? [s] : [] }
    return
  }
  if (lastSeen === appVersion) return
  const sections = unseenSections(changelog, lastSeen, appVersion)
  try {
    await versionSeen(appVersion)
  } catch {
    return // outside Castle's window
  }
  if (sections.length) whatsNew.value = { version: appVersion, sections }
}

function showWhatsNew() {
  const s = sectionFor(changelog, appVersion)
  whatsNew.value = { version: appVersion, sections: s ? [s] : [] }
}

async function loadSettingsAndTheme() {
  let wanted = DEFAULT_THEME_ID
  let lastSeen = null
  loaderBegin('settings', 'Reading your settings', 'settings.json')
  try {
    const view = await getSettings()
    lastSeen = view.settings.lastSeenVersion ?? null
    offerFacts.value = { offerSeen: !!view.settings.dbOfferSeen, hadCastleBefore: lastSeen !== null, hasFolder: !!view.settings.memoryFolder }
    companion.value = view.companionLabel
    cli.value = view.settings.cli
    memoryFolder.value = view.settings.memoryFolder ?? ''
    memberName.value = view.settings.memberName ?? ''
    rooms.value = view.settings.rooms ?? {}
    barClock.value = { enabled: !!view.settings.displayClock, format: view.settings.clockFormat ?? '24h' }
    usageCard.value = view.settings.showUsageCard !== false
    wallpaperMotion.value = view.settings.livingWallpaper !== false
    wallpaperSound.value = view.settings.wallpaperSound !== false
    widgetPrefs.value = { widgetsOff: view.settings.widgetsOff ?? [], widgetsOn: view.settings.widgetsOn ?? [], widgetSizes: view.settings.widgetSizes ?? {}, widgetPlaces: view.settings.widgetPlaces ?? {} }
    tabOrder.value = view.settings.tabOrder ?? []
    pins.value = cleanPins(view.settings.pinnedRooms ?? DEFAULT_PINS)
    try {
      savedKeys.value = (await keyStatus()).filter((k) => k.set).map((k) => k.name)
    } catch {
      // the keys stay as they were
    }
    if (!view.settings.setupDone) {
      setupOpen.value = true
      setupReopened.value = false
    }
    wanted = view.settings.theme ?? DEFAULT_THEME_ID
  } catch {
    // Outside Castle's window (a browser preview): defaults.
  }
  loaderEnd('settings')
  loaderBegin('themes', 'Choosing your theme', `themes/${wanted}`)
  let folders = []
  try {
    folders = await loadUserThemes()
  } catch {
    // Built-in themes only.
  }
  // dev only: ?theme=<id> shows a theme in a browser check
  const devTheme = import.meta.env.DEV ? new URLSearchParams(location.search).get('theme') : null
  if (devTheme) wanted = devTheme
  const all = [...builtInThemes(), ...fromUserFolders(folders).themes]
  const chosen = chooseTheme(all, wanted)
  theme.value = chosen.theme
  themeNotice.value = chosen.notice
  applyTheme(chosen.theme)
  loaderRemember(chosen.theme)
  loaderEnd('themes')
  if (!setupOpen.value) checkWhatsNew(lastSeen)
}

function openSettings(section = 'companion') {
  settingsSection.value = typeof section === 'string' ? section : 'companion'
  settingsOpen.value = true
}

// A tab dragged into a new place (or moved with Alt+arrow): the order covers every tab, the hidden
// rooms too, so a room switched back on returns to where the member left it.
async function reorderTabs({ from, to }) {
  const ids = orderTabs(allTabs, tabOrder.value).map((t) => t.id)
  const next = moveTab(ids, from, to)
  if (next === ids) return
  tabOrder.value = next
  try {
    const { settings } = await getSettings()
    await saveSettings({ ...settings, tabOrder: next })
  } catch {
    // outside Castle's window: the order holds until the page reloads
  }
}

// Esc in APPS: back to the room it was opened from (HOME if that room is gone).
function closeLauncher() {
  active.value = tabs.value.some((t) => t.id === previousRoom.value) ? previousRoom.value : 'home'
}
// Ctrl+K (Cmd+K on a Mac) opens APPS from anywhere, and closes it again. DEV is left alone: in a terminal
// Ctrl+K is the shell's own "delete to the end of the line", and a sheet over Castle keeps the keyboard.
function onLauncherKey(event) {
  if (!opensLauncher(event, { room: active.value, sheetOpen: settingsOpen.value || sentinelOpen.value || backupOpen.value })) return
  event.preventDefault()
  // In APPS, Ctrl+K puts the cursor in the search box (Esc is what leaves APPS); from another room it opens APPS with the cursor there.
  if (active.value === 'launcher') launcherView.value?.focusSearch()
  else { launcherSearch.value = true; active.value = 'launcher' }
}

// A room opened from APPS. A pinned room is just a tab; any other room shows as a tab while it is open.
function openRoom(id) {
  active.value = id
  marks.value = marks.value.filter((m) => m !== id)
}

// The pin on a tile (or in Settings): the room joins or leaves the top bar.
async function pinRoom({ id, on }) {
  const next = setPinned(pins.value, id, on, allTabs.map((t) => t.id))
  pins.value = next
  try {
    const { settings } = await getSettings()
    await saveSettings({ ...settings, pinnedRooms: next })
  } catch {
    // outside Castle's window: the pins hold until the page reloads
  }
}

function openKeys() {
  openSettings('keys')
}

// THEME's own saves (the main theme, and the diary/story/profile looks) go through here too, so
// every settingsStamp listener (HOME's PROFILE tab among them) hears about them the same as a
// save from Settings does.
async function themeChanged() {
  await loadSettingsAndTheme()
  settingsStamp.value += 1
}

async function settingsSaved() {
  await loadSettingsAndTheme()
  settingsStamp.value += 1
  // the database may have been installed, started or moved into from Settings
  loadDatabase()
  loadHome()
}

function openSetup() {
  setupReopened.value = true
  setupOpen.value = true
}

async function setupDone() {
  setupOpen.value = false
  // the setup's last step already showed the database panel: the one-time offer is not asked again
  try {
    const { settings } = await getSettings()
    if (!settings.dbOfferSeen) await saveSettings({ ...settings, dbOfferSeen: true })
  } catch {
    // outside Castle's window
  }
  await loadSettingsAndTheme()
  settingsStamp.value += 1
}

// The member's local date, so "days" count in their own time.
function today() {
  const d = new Date()
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`
}

async function loadHome() {
  memoryStamp.value += 1
  try {
    home.value = await memoryHome(today())
    avatar.value = await avatarGet()
  } catch {
    // outside Castle's window
  }
  // a hidden room may not stay open
  if (active.value !== 'launcher' && !tabs.value.some((t) => t.id === active.value)) active.value = 'home'
}

async function loadDatabase() {
  try {
    const status = await dbStatus()
    databaseBytes.value = status.downloadBytes
    databaseSource.value = status.source ?? 'download'
    database.value = { engine: status.engine, memory: await dbMemory() }
  } catch {
    database.value = null
    return
  }
  // asked once, after "What's new" has had its turn
  if (!whatsNew.value && !databaseOffer.value && shouldOffer({ ...offerFacts.value, setupOpen: setupOpen.value }, database.value.engine, database.value.memory)) {
    databaseOffer.value = true
  }
}

function closeDatabaseOffer() {
  databaseOffer.value = false
  offerFacts.value = { ...offerFacts.value, offerSeen: true }
  loadDatabase()
  loadHome()
}

// The DATABASE card: Start runs here; everything else opens Settings > Database.
async function onDatabaseCard(action) {
  if (action === 'start') {
    try {
      await dbStart()
    } catch {
      openSettings('database')
    }
    await loadDatabase()
    loadHome()
    return
  }
  openSettings('database')
}

async function loadBackup() {
  try {
    backup.value = await backupStatus()
  } catch {
    backup.value = null
  }
}

// A project or diary card opens its page in the DIARY or PROJECTS room.
function openMemoryFile(file) {
  if (!file) return
  // a page of the database opens under db/project/... or db/diary/...
  const room = file.startsWith('projects/') || file.startsWith('db/project/') ? 'projects' : 'diary'
  if (!tabs.value.some((t) => t.id === room)) return
  roomFile.value = file
  active.value = room
}

async function onBackupCard(mode) {
  if (mode === 'setup') {
    backupOpen.value = true
    return
  }
  try {
    backup.value = await backupNow()
  } catch {
    backupOpen.value = true
  }
}

// Castle's own Node, when the computer has none new enough: the start-up screen shows the download
async function prepareHelpers() {
  const mb = (bytes) => (bytes / 1048576).toFixed(1)
  const stop = await onNodeInstall(({ done, total }) => loaderBegin('helpers', 'Downloading the helpers once', `${mb(done)} / ${mb(total)} MB`)).catch(() => null)
  try {
    await nodePrepare()
  } finally {
    stop?.()
  }
}

let refresh
let stopBackupEvents
let stopStoryEvents
let stopProviderEvents
let stopOpenChat
let stopOpenRoom
onMounted(async () => {
  window.addEventListener('keydown', onArrangeKey)
  window.addEventListener('keydown', onLauncherKey)
  stopProviderEvents = await onProviderChanged(async () => { await settingsSaved(); window.dispatchEvent(new Event('castle-provider-changed')) }).catch(() => null)
  stopOpenChat = await onOpenChat(() => {
    active.value = 'chat'
    marks.value = marks.value.filter((m) => m !== 'chat')
  }).catch(() => null)
  // a notification clicked: its room, if that room is on offer
  stopOpenRoom = await onOpenRoom((room) => {
    if (typeof room === 'string' && tabs.value.some((t) => t.id === room)) {
      active.value = room
      marks.value = marks.value.filter((m) => m !== room)
    }
  }).catch(() => null)
  alertActiveRoom(active.value).catch(() => {})
  // the start-up screen reports what is loading, then goes when it is all in (or after a wait too long)
  // a room switched off is not prepared; settings and the theme are two pieces, then one per room
  await loadSettingsAndTheme()
  const roomOn = (id) => rooms.value[id] !== false
  const rooms_ = ['studio', 'library', 'appearance', 'world', 'automations', 'calendar'].filter(roomOn)
  loaderExpect(2 + 4 + rooms_.length)
  // each piece has its own limit, so one slow piece (the internet, a first run) never holds the rest
  const piece = (id, label, file, work, limit = 25000) => {
    loaderBegin(id, label, file)
    const work_ = Promise.resolve().then(work).catch(() => {})
    return Promise.race([work_, new Promise((resolve) => setTimeout(resolve, limit))]).finally(() => loaderEnd(id))
  }
  const folder = memoryFolder.value.split(String.fromCharCode(92)).join('/').split('/').filter(Boolean).pop() ?? ''
  const loading = Promise.allSettled([
    piece('memory', 'Reading your memory', folder, loadHome),
    piece('backup', 'Checking the backup', 'backup status', loadBackup),
    piece('database', 'Waking the memory database', 'memory database', loadDatabase),
    // the helpers' Node: the member's own when new enough, else Castle's own, downloaded once; a
    // download still running when the screen leaves carries on, and the helpers work once it lands
    piece('helpers', 'Getting the helpers ready', 'Node', prepareHelpers, 20000),
    roomOn('studio') && piece('studio', "Preparing STUDIO's gallery", 'media-generation/', () => prepareStudio((done, total) => loaderBegin('studio', 'Making small copies of your pictures', `${done} / ${total}`))),
    roomOn('appearance') && piece('appearance', 'Hanging up the wardrobe', 'appearance/', () => prepareAppearance((done, total) => loaderBegin('appearance', 'Making small copies of the wardrobe', `${done} / ${total}`))),
    roomOn('world') && piece('world', 'Counting the screens and worlds', 'screens and worlds', prepareWorld, 8000),
    roomOn('automations') && piece('automations', 'Preparing AUTOMATIONS', 'scheduled jobs', prepareAutomations, 8000),
    roomOn('calendar') && piece('calendar', 'Preparing CALENDAR', 'dated things', prepareCalendar, 8000),
    roomOn('library') && piece('library', "Opening LIBRARY's shelves", 'my books and the Spiral Library', prepareLibrary, 8000),
  ].filter(Boolean))
  // long enough for a first start to make a large gallery's small copies; after that it is quick
  await Promise.race([loading, new Promise((resolve) => setTimeout(resolve, 25000))])
  loaderDone()
  // the memory changes as the companion writes: read it again every minute
  const automationRefresh = () => { if (rooms.value.automations) prepareAutomations().catch(() => {}) }; automationRefresh()
  refresh = setInterval(() => { loadHome(); if (rooms.value.automations) prepareAutomations().catch(() => {}); if (rooms.value.calendar) refreshCalendarCard(); refreshPending() }, 60000)
  try {
    stopAutomations = await onAutomations(() => { if (rooms.value.automations) prepareAutomations().catch(() => {}) })
    stopCalendar = await onCalendar(() => { if (rooms.value.calendar) refreshCalendarCard() })
    stopBackupEvents = await onBackupDone(() => loadBackup())
    stopStoryEvents = await onStoryOutput(() => {
      if (active.value !== 'story' && !marks.value.includes('story')) marks.value = [...marks.value, 'story']
    })
  } catch {
    // outside Castle's window
  }
})
onBeforeUnmount(() => {
  window.removeEventListener('keydown', onArrangeKey)
  window.removeEventListener('keydown', onLauncherKey)
  stopAutomations?.()
  stopCalendar?.()
  stopProviderEvents?.()
  stopOpenChat?.()
  stopOpenRoom?.()
  clearInterval(refresh)
  stopBackupEvents?.()
  stopStoryEvents?.()
})
watch(settingsStamp, () => {
  loadHome()
  loadBackup()
  loadDatabase()
})
</script>

<template>
  <div class="castle">
    <TopBar
      :tabs="barTabs"
      :active="active"
      :display-clock="barClock.enabled"
      :version="appVersion"
      :clock-format="barClock.format"
      :marks="barMarks"
      :mark-titles="{ story: 'The story has new lines', chat: 'New in the room', launcher: 'Something new is waiting in a room' }"
      @select="active = $event; marks = marks.filter((m) => m !== $event)"
      @reorder="reorderTabs"
      @settings="openSettings"
      @sentinel="sentinelOpen = true"
      @hide="hideCastle().catch(() => {})"
    />
    <main class="screen">
      <div v-show="active === 'home'" ref="homeEl" class="home" :class="[theme?.widgetStyle ?? 'glass', skinClass(theme), { arranging, still: !wallpaperMotion || active !== 'home' }]">
        <UsageReveal v-if="theme?.usageReveal" :theme="theme" :visible="active === 'home'" />
        <ThemeMotion v-if="theme?.motion" :theme="theme" :visible="active === 'home'" :enabled="wallpaperMotion" />
        <ThemeVideo v-if="theme?.motionVideoUrl" :theme="theme" :visible="active === 'home'" :enabled="wallpaperMotion" />
        <ThemeSound v-if="theme?.motionSoundUrl" :theme="theme" :visible="active === 'home'" :enabled="wallpaperSound" />
        <QuotaGlow v-if="theme?.quotaGlow" :theme="theme" :visible="active === 'home'" />
        <TerminalPanel
          :settings-stamp="settingsStamp"
          :companion="companion"
          :cli="cli"
          @settings="openSettings"
          @setup="openSetup"
        >
          <template #header="{ running, busy }"><CompanionHeader :name="companion" :avatar="avatar" :running="running" :busy="busy" /></template>
        </TerminalPanel>
        <div
          v-for="id in shownWidgets"
          :key="id"
          class="widget-slot"
          :data-widget="id"
          :class="{ movable: arranging, moving: drag?.id === id }"
          :style="slotStyle(id)"
          @pointerdown="startDrag($event, id)"
          @pointermove="moveDrag"
          @pointerup="endDrag"
          @pointercancel="endDrag"
        >
          <ClockWidget v-if="id === 'clock'" :size="widgets.clock?.size ?? 'm'" :format="barClock.format" />
          <GaugeWidget v-else-if="id === 'gauges'" />
          <WeatherWidget v-else-if="id === 'weather'" />
          <DisksWidget v-else-if="id === 'disks'" />
          <SentinelWidget v-else-if="id === 'sentinel'" @open="sentinelOpen = true" />
          <AutomationsWidget v-else-if="id === 'automations'" :data="automations" @open="active = 'automations'" />
          <CalendarWidget v-else-if="id === 'calendar'" :data="calendarCard" @open="active = 'calendar'" />
          <SweepWidget v-else-if="id === 'sweep'" :theme-look="theme?.sweep?.look ?? ''" />
          <UsageWidget v-else-if="id === 'usage'" :stamp="settingsStamp" />
          <CreditWidget v-else-if="isCreditWidget(id)" :widget="id" />
          <HomeCards v-else :kind="id" :data="home" :backup="backup" :database="database" @open="openMemoryFile" @backup="onBackupCard" @database="onDatabaseCard" />
        </div>
        <button v-if="!arranging" class="manage-widgets" @click="openSettings('widgets')">Manage widgets</button>
        <div v-if="arranging" class="arrange-bar" role="toolbar" aria-label="Arrange HOME">
          <span>Drag a widget to move it. It stays where you put it, whatever the theme.</span>
          <button @click="saveWidgets(resetPatch()).then(keepPlacesSoon)">Reset the layout</button>
          <button class="primary" @click="arranging = false">Done</button>
        </div>
      </div>
      <AppsView v-if="active === 'apps'" class="room" />
      <MyProfileView v-if="active === 'myprofile'" :companion="companion" @settings="openSettings('database')" />
      <ChatRoomView v-show="active === 'chat'" v-if="chatOpened" :visible="active === 'chat'" :companion="companion" :settings-stamp="settingsStamp" @settings="openSettings" @renamed="settingsSaved" @fresh="!marks.includes('chat') && (marks = [...marks, 'chat'])" />
      <DevView v-show="active === 'dev'" v-if="devOpened" :visible="active === 'dev'" :companion="companion" :cli="cli" :settings-stamp="settingsStamp" @settings="openSettings" @setup="openSetup" />
      <WorldView v-if="active === 'world'" :active="active === 'world'" :companion="companion" @show-chat="active = 'home'" />
      <StoryView v-show="active === 'story'" v-if="storyOpened" :companion="companion" :member="memberName" :cli="cli" :visible="active === 'story'" />
      <AppearanceView v-if="active === 'appearance'" :companion="companion" :stamp="memoryStamp" @keys="openKeys" />
      <StudioView v-if="active === 'studio'" :companion="companion" :stamp="memoryStamp" @keys="openKeys" />
      <ThemeView v-if="active === 'theme'" :companion="companion" @changed="themeChanged" @show-chat="active = 'home'" />
      <MemoryRoom v-if="['diary', 'projects'].includes(active)" :room="active" :stamp="memoryStamp" :open-file="roomFile" />
      <LibraryView v-if="active === 'library'" :companion="companion" :stamp="memoryStamp" />
      <MarketView v-if="active === 'market'" :companion="companion" />
      <AutomationsView v-if="active === 'automations'" @settings="openSettings" />
      <LauncherView v-if="active === 'launcher'" ref="launcherView" :search-on-open="launcherSearch" :tabs="tabs" :pins="pins" :badges="badges" :current="active" @open="openRoom" @pin="pinRoom" @close="closeLauncher" />
      <CalendarView v-if="active === 'calendar'" @open="(room) => (active = room)" />
      <MessagingView v-if="active === 'messaging'" :companion="companion" />
      <div v-if="!BUILT.has(active)" class="room-soon">
        <p>{{ activeLabel }}</p>
      </div>
      <p v-if="themeNotice" class="theme-notice">{{ themeNotice }}</p>
      <SettingsView
        v-if="settingsOpen"
        :section="settingsSection"
        :theme-id="theme?.id ?? ''"
        :theme-name="theme?.name ?? ''"
        :theme-widgets="widgets"
        @arrange="settingsOpen = false; arrange()"
        :version="updateInfo?.version ?? ''"
        :update-line="updateInfo?.message ?? ''"
        @close="settingsOpen = false"
        @saved="settingsSaved"
        @setup="settingsOpen = false; openSetup()"
        @whatsnew="showWhatsNew"
        @open-room="(room) => { settingsOpen = false; if (tabs.some((t) => t.id === room)) active = room }"
      />
      <WhatsNew v-if="whatsNew" :version="whatsNew.version" :sections="whatsNew.sections" @close="whatsNew = null; loadDatabase()" />
      <DatabaseOffer v-if="databaseOffer" :companion="companion" :download-bytes="databaseBytes" :source="databaseSource" @close="closeDatabaseOffer" @changed="loadHome" />
      <ModalSheet v-if="backupOpen" tag="BACKUP" title="Keep the memory safe" @close="backupOpen = false; loadBackup()">
        <BackupPanel :companion="companion" />
      </ModalSheet>
      <ModalSheet v-if="sentinelOpen" tag="SENTINEL" title="Keeping watch" @close="sentinelOpen = false">
        <SentinelSheet @settings="sentinelOpen = false; openSettings('sentinel')" />
      </ModalSheet>
      <NoticeToasts @keys="openKeys" />
      <MeterAlert />
      <AmbientMusic />
      <UpdateChip @status="updateInfo = $event" @about="openSettings('about')" />
      <SetupView v-if="setupOpen" :reopened="setupReopened" @done="setupDone" @close="setupOpen = false" />
    </main>
  </div>
</template>

<style>
:root {
  --bar-height: 48px;
  --background: #050b1f;
  --bar-background: rgba(6, 12, 34, 0.92);
  --line: rgba(150, 190, 255, 0.34);
  --text: #eef3ff;
  --text-dim: #a6b8de;
  --accent: #8fb8ff;
  --hover: rgba(150, 190, 255, 0.14);
  --panel-background: rgba(10, 20, 52, 0.78);
  --widget-background: rgba(12, 24, 60, 0.62);
  --glass-blur: 14px;
  --good: #8fe3b8;
  --warn: #ffd59a;
  --bad: #ff9aa8;
  --wallpaper: none;
  --wallpaper-position: center;
  /* Native controls follow the theme. A theme may set any of these in its "colors" like any other
     variable; none is needed, the defaults below read the theme's own colours. */
  --scrollbar-thumb: var(--line); /* the draggable part of every scrollbar */
  --scrollbar-track: transparent; /* the groove behind it */
  --scrollbar-width: thin; /* thin, auto (wider) or none (hidden) */
  --control-accent: var(--accent); /* checkboxes, radio buttons and sliders */
  --color-scheme: dark; /* dark or light: native pickers, spin buttons and text selection */
}

:root {
  color-scheme: var(--color-scheme);
  accent-color: var(--control-accent);
}

/* every scroller, also the ones no component styles itself */
* {
  scrollbar-width: var(--scrollbar-width);
  scrollbar-color: var(--scrollbar-thumb) var(--scrollbar-track);
}

html,
body,
#app {
  height: 100%;
  margin: 0;
  overflow: hidden;
}

body {
  background: var(--background);
  color: var(--text);
  font-family: -apple-system, 'Segoe UI', system-ui, sans-serif;
}

.castle {
  display: flex;
  flex-direction: column;
  height: 100%;
  background-color: var(--background);
  background-image: var(--wallpaper);
  background-position: var(--wallpaper-position);
  background-size: cover;
  background-repeat: no-repeat;
}

.screen {
  position: relative;
  flex: 1;
  min-height: 0;
}

.home {
  position: absolute;
  inset: 0;
}

.widget-slot {
  position: absolute;
  transform-origin: top left;
}

/* Arrange mode: every widget shows its edge and follows the pointer; nothing inside is pressed. */
.widget-slot.movable {
  cursor: grab;
  outline: 2px dashed color-mix(in srgb, var(--accent) 70%, transparent);
  outline-offset: 4px;
  border-radius: 14px;
  touch-action: none;
}

.widget-slot.movable > * {
  pointer-events: none;
}

.widget-slot.moving {
  cursor: grabbing;
  user-select: none;
  z-index: 5;
  opacity: 0.9;
}

.manage-widgets {
  position: absolute;
  right: 18px;
  bottom: 18px;
  z-index: 6;
  font: inherit;
  font-size: 0.85rem;
  color: var(--text);
  background: var(--panel-background);
  border: 1px solid var(--line);
  border-radius: 8px;
  padding: 8px 12px;
  cursor: pointer;
}

.arrange-bar {
  position: absolute;
  left: 50%;
  bottom: 18px;
  transform: translateX(-50%);
  z-index: 6;
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 14px;
  background: var(--panel-background);
  border: 1px solid var(--line);
  border-radius: 12px;
  -webkit-backdrop-filter: blur(var(--glass-blur)); backdrop-filter: blur(var(--glass-blur));
  color: var(--text);
  font-size: 0.9rem;
}

.arrange-bar button {
  font: inherit;
  color: var(--text);
  background: var(--hover);
  border: 1px solid var(--line);
  border-radius: 8px;
  padding: 6px 12px;
  cursor: pointer;
}

.arrange-bar button.primary {
  background: var(--accent);
  color: #07142e;
  border-color: transparent;
}

/* Widget frames: glass themes give them a pane, bare themes let the wallpaper show through. */
.home.glass .widget {
  background: var(--widget-background);
  border: 1px solid var(--line);
  border-radius: 14px;
  -webkit-backdrop-filter: blur(var(--glass-blur)); backdrop-filter: blur(var(--glass-blur));
}

.home.bare .widget {
  background: transparent;
  text-shadow: 0 2px 10px rgba(0, 0, 0, 0.55);
}

.room {
  position: absolute;
  inset: 0;
}

.room-soon {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  color: var(--text-dim);
  letter-spacing: 0.3em;
}

.theme-notice {
  position: absolute;
  bottom: 12px;
  left: 50%;
  margin: 0;
  padding: 6px 14px;
  font-size: 13px;
  color: #07142e;
  background: var(--warn);
  border-radius: 8px;
  transform: translateX(-50%);
}
</style>

<style scoped>
</style>
