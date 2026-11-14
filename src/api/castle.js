// The only file that talks to Castle's Rust core.
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { typeLine } from '../components/typeline.js'
import { devSlotOf, pasted } from '../views/dev/devtabs.js'

export const hideCastle = () => invoke('hide_castle')

// Settings. -> { settings, companionLabel, cliLabel, warning, startupNote }
export const getSettings = () => invoke('get_settings')
export const saveSettings = (settings) => invoke('save_settings', { settings })
export const chooseFolder = (title) => invoke('choose_folder', { title })

// First-run setup. -> FolderState { exists, isMemorycore, needsNames, entryFile, companionName }
export const setupInspect = (folder) => invoke('setup_inspect', { folder })
export const setupCreate = (folder) => invoke('setup_create', { folder })
// -> { cli, label, installed, version, signedIn, installCommand, signInHint }
export const setupCheckCli = (cli) => invoke('setup_check_cli', { cli })
export const setupApply = (folder, companion, member, style) => invoke('setup_apply', { folder, companion, member, style })
// Keys the member already keeps in the memory folder's .env (masked), and keeping them in Castle.
export const setupFoundKeys = (folder) => invoke('setup_found_keys', { folder })
export const setupImportKeys = (folder, names) => invoke('setup_import_keys', { folder, names })
export const choosePicture = (title) => invoke('choose_picture', { title })
// The companion's picture (a data URL), or null for the default crystal.
export const avatarSet = (path) => invoke('avatar_set', { path })
export const avatarGet = () => invoke('avatar_get')
export const avatarClear = () => invoke('avatar_clear')

// The private GitHub backup. -> BackupStatus { gitInstalled, ghInstalled, ghSignedIn, ghUser, isRepo, remoteUrl,
//   remoteIsTemplate, remoteOwned, visibility, changed, ahead, lastBackup, ready, nextStep }
export const backupStatus = () => invoke('backup_status')
export const backupCreate = (name) => invoke('backup_create', { name })
export const backupNow = () => invoke('backup_now')
export const backupSignIn = () => invoke('backup_sign_in')
export const openUrl = (url) => invoke('open_url', { url })
export const onBackupDone = (handler) => listen('backup-done', (event) => handler(event.payload))

// Keys, kept encrypted by Windows. -> [{ name, label, set, masked }]
export const keyStatus = () => invoke('key_status')
export const keySave = (name, value) => invoke('key_save', { name, value })
export const keyForget = (name) => invoke('key_forget', { name })
// The whole key, only when the member presses Show.
export const keyReveal = (name) => invoke('key_reveal', { name })
// Checks a key with the service's free account endpoint. -> { ok, message }
export const keyTest = (name) => invoke('key_test', { name })

// The companion's sessions. room: undefined for the main session, 'story' for the STORY room's.
// -> { running, status: { ready, message, fix }, snapshot }
export const terminalState = (room) => invoke('terminal_state', { room })
export const providersList = () => invoke('providers_list')
export const imageJobs = () => invoke('image_jobs')
export const imageRetry = (id) => invoke('image_retry', { id })
// Takes finished requests off the list: the one with this id, or all of them with no id.
export const imageDismiss = (id = null) => invoke('image_dismiss', { id })
export const imageCancel = (id) => invoke('image_cancel', { id })
// `label`: what the picture is for, shown in the Image requests list ("Try-on: Winter coat + Twin tails").
export const imageRequest = (brief, output = null, references = [], room = 'studio', label = null) => invoke('image_request', { brief, output, references, room, label })
export const providerSwitch = (target) => invoke('provider_switch', { target })
export const providerSwitchStatus = () => invoke('provider_switch_status')
export const providerSwitchCancel = () => invoke('provider_switch_cancel')
export const onProviderChanged = (handler) => listen('provider-changed', handler)
// A DEV tab's chat uses the room `dev:<slot>` (devRoom): its history, reads, sends and media are routed
// to the dev_* commands below; every other room goes to the core exactly as before.
export const chatHistory = (room) => (devSlotOf(room) !== null ? Promise.resolve([]) : invoke('chat_history', { room }))
export const sendMessage = (text, room) => {
  const slot = devSlotOf(room)
  if (slot === null) return invoke('send_message', { text, room })
  // as typed: one bracketed paste, then Enter after a pause (Codex needs it)
  return typeLine((data) => devWrite(slot, data), pasted(text)).then(() => ({ queued: false }))
}
export const queuedMessages = () => invoke('queued_messages')
export const queuedMessageAction = (id, retry = false) => invoke('queued_message_action', { id, retry })
// `greet`: only the Wake button asks for it - Castle then opens the conversation so the companion greets.
export const terminalStart = (cols, rows, room, greet = false) => invoke('terminal_start', { cols, rows, room, greet })
export const terminalWrite = (data, room) => invoke('terminal_write', { data, room })
export const terminalResize = (cols, rows, room) => invoke('terminal_resize', { cols, rows, room })
export const storyClose = () => invoke('story_close')
// True once, when Windows started Castle and the start-session switch is on.
export const takeAutoWake = () => invoke('take_auto_wake')

// Chat: new entries from the session's own record. -> { entries: [{ id, role, text, at, path, pictures, state }], reset, status }
// A DEV tab's chat is read only while that tab shows (devChatShown); a hidden one answers nothing yet.
export const chatPoll = (room) => {
  const slot = devSlotOf(room)
  if (slot === null) return invoke('chat_poll', { room })
  return devShownSlots.has(slot) ? devChatPoll(slot) : Promise.reject(new Error('not shown'))
}
// A picture or voice Chat showed this session, as a data URL. A DEV tab's chat asks its own door
// (only what that tab's reader showed, or a picture the member dropped), and remembers which tab
// showed the file so the picture viewer's Open goes to the same door.
const devMediaSlots = new Map()
export const chatMedia = (path, room) => {
  const slot = devSlotOf(room)
  if (slot === null) return invoke('chat_media', { path, room })
  devMediaSlots.set(path, slot)
  return invoke('dev_chat_media', { slot, path })
}
// Pictures the member dropped or pasted; returns the ones Chat may show.
export const chatAttach = (paths) => invoke('chat_attach', { paths })

// Which MemoryCore features the member's folder holds. -> { diary, projects, library, saveMemory, ... }
export const memoryFeatures = () => invoke('memory_features')
// Everything HOME shows. today = the member's local date (YYYY-MM-DD). -> { features, reminders, projects, diary, growth } or null
export const memoryHome = (today) => invoke('memory_home', { today })
// A room's shelves (room: diary | projects | library) -> [{ name, pages: [{ title, file, date }] }]
export const memoryShelves = (room) => invoke('memory_shelves', { room })
// One memory page's markdown (only .md inside the memory folder).
export const memoryRead = (file) => invoke('memory_read', { file })

// The desktop.
export const systemMeters = () => invoke('system_meters')
export const appsList = () => invoke('apps_list')
export const appsDrawIcons = () => invoke('apps_draw_icons')
export const appOpen = (path) => invoke('app_open', { path })
export const clipboardPaste = () => invoke('clipboard_paste')
export const clipboardCopy = (text) => invoke('clipboard_copy', { text })
export const userThemes = () => invoke('user_themes')
// Create theme: a draft from a picture (or a gradient) -> the new theme's id.
export const themeCreate = (name, mood, picture) => invoke('theme_create', { name, mood, picture })
export const themeDelete = (id) => invoke('theme_delete', { id })
export const themeFolder = (id) => invoke('theme_folder', { id })
// Opens a file from the memory folder in its own app, or its folder with the file selected. A file a
// DEV tab's chat showed (by `room`, or remembered from chatMedia) opens through that tab's door.
export const chatMediaOpen = (path, folder = false, room = undefined) => {
  const slot = devSlotOf(room) ?? devMediaSlots.get(path) ?? null
  if (slot === null) return invoke('media_open', { path, folder })
  return invoke('dev_media_open', { slot, path, folder })
}

// Events from the core.
const on = (name) => (handler) => listen(name, (event) => handler(event.payload))
export const onTerminalOutput = on('terminal-output')
export const onTerminalExit = on('terminal-exit')
export const onStoryOutput = on('story-output')
export const onStoryExit = on('story-exit')
// APPEARANCE. -> { folder, canon, portrait, wardrobe: [{ id, name, note, sheet, savedPrompt }], hairstyles, current: { outfit, hair } }
export const appearanceLook = () => invoke('appearance_look')
export const appearanceNew = (kind, name, description) => invoke('appearance_new', { kind, name, description })
export const appearanceImport = (kind, id, source) => invoke('appearance_import', { kind, id, source })
export const appearanceWear = (outfit, hair) => invoke('appearance_wear', { outfit, hair })

// STORY: adventures/ prepared with Castle's rulebook; the saved worlds.
export const storyPrepare = () => invoke('story_prepare')
// -> [{ id, name, status, chapter, parts, summary, scenes }]
export const storyWorlds = () => invoke('story_worlds')
export const storyBeat = (world) => invoke('story_beat', { world })

// WORLD on the other screens. -> [{ index, name, width, height, isMain, open }]
export const worldScreens = () => invoke('world_screens')
export const worldApply = (screens) => invoke('world_apply', { screens })
export const worldIdentify = (index) => invoke('world_identify', { index })
export const castleMoveToScreen = (index) => invoke('castle_move_to_screen', { index })
export const worldMusic = (patch) => invoke('world_music', { music: patch.music ?? null, volume: patch.volume ?? null })
export const roomsApply = (rooms) => invoke('rooms_apply', { rooms })
export const worldsMine = () => invoke('worlds_mine')
export const cliQuota = () => invoke('cli_quota')
// Both CLIs' five-hour and weekly limits for HOME's USAGE card.
export const cliUsage = () => invoke('cli_usage')
export const worldNew = (name, description, layout, picture) => invoke('world_new', { name, description, layout, picture })
export const worldDelete = (slug) => invoke('world_delete', { slug })
export const worldPrepare = (slug) => invoke('world_prepare', { slug })

// Signing in to the MemoryCore Foundation through the browser (the token stays in the core).
export const memberSignIn = () => invoke('member_sign_in')
export const memberStatus = () => invoke('member_status')
export const memberSignOut = () => invoke('member_sign_out')
export const roomCompanionIn = (on) => invoke('room_companion_in', { on })

// The MemoryCore Foundation room (keys stay in the core). Refusals come back as { kind, message }.
export const roomMe = (who) => invoke('room_me', { who })
export const roomLines = (after, wait) => invoke('room_lines', { after, wait })
export const roomPresence = () => invoke('room_presence')
export const roomPost = (text, who) => invoke('room_post', { text, who })
export const roomFile = (id) => invoke('room_file', { id })
export const roomSeen = () => invoke('room_seen')
export const roomAvatar = (token) => invoke('room_avatar', { token })
export const roomMine = () => invoke('room_mine')
// path = a picture the member chose, or null for back to their Google picture; companion = its picture
export const roomFaceSet = (path, companion = false) => invoke('room_face_set', { path, companion })
export const roomNameSet = (name) => invoke('room_name_set', { name })
// after the companion's name or picture changes in Settings: the room is brought up to date
// ({ inRoom, renamed, face }; inRoom false = the companion is not in the room, nothing to do)
export const roomCompanionRefresh = () => invoke('room_companion_refresh')
// the Neural Link, read-only: { on, here: [{ terminal, place, seen }], notes: [{ from, to, at, about, wake }] }
export const linkView = () => invoke('link_view')
// Castle's room reader (it reads from the moment Castle starts)
export const roomBacklog = () => invoke('room_backlog')
export const roomVisible = (on) => invoke('room_visible', { on })
export const roomReset = () => invoke('room_reset')
// the companion's room turns and its public card
export const roomLog = () => invoke('room_log')
export const roomCard = () => invoke('room_card')
export const roomCardSave = (text) => invoke('room_card_save', { text })

// Updates (official builds only). -> { official, version, available, notes, message }
export const updateCheck = () => invoke('update_check')
export const updateInstall = () => invoke('update_install')
// remembers the version whose "What's new" was shown; answers the one seen before
export const versionSeen = (version) => invoke('version_seen', { version })
// where "All releases" opens (the Foundation's page once it answers, else GitHub's)
export const releasesPage = () => invoke('releases_page')
// The diary's newest change (seconds), to wait for Save diary before an update.
export const diaryStamp = () => invoke('diary_stamp')

// The memory database. dbStatus -> { engine: { state: notInstalled | stopped | running | external, port, external, answering },
//   version, downloadBytes, keepRunning }; dbMemory -> { name, reachable, movedIn, canRestore };
//   dbMovePreview / dbMoveIn -> { files, diary, reminders, projects, memory, skills, skipped }
export const dbStatus = () => invoke('db_status')
export const dbInstall = () => invoke('db_install')
export const dbStart = () => invoke('db_start')
export const dbStop = () => invoke('db_stop')
export const dbUseExisting = () => invoke('db_use_existing')
export const dbCreate = (adminUser, adminPassword) => invoke('db_create', { adminUser, adminPassword })
export const dbMemory = () => invoke('db_memory')
export const dbMovePreview = () => invoke('db_move_preview')
export const dbMoveIn = () => invoke('db_move_in')
export const dbRestore = () => invoke('db_restore')
export const dbExport = (to) => invoke('db_export', { to })
// the way back: the memory becomes files in its folder again and the database is emptied
export const dbMoveOut = () => invoke('db_move_out')
// the memory's skills -> [{ plugin, name, description, stubbed }]; a stubbed skill keeps a small
// SKILL.md on disk and its instructions are loaded from the database when it fires
export const dbSkills = () => invoke('db_skills')
export const dbSkillStub = (plugin, name, on) => invoke('db_skill_stub', { plugin, name, on })
// "Set it up for me": install, start, create, check - then it stops and gives the move's counts
export const dbSetupAll = () => invoke('db_setup_all')
// while it runs: { step: install | start | create | check, state: running | done }
export const onDbSetup = on('castle://db-setup')
// while the engine downloads: { done, total } in bytes
export const onDbInstall = on('castle://db-install')

// STUDIO: what the companion made, and each power's state (ready | builtin | prompt).
export const studioLibrary = () => invoke('studio_library')
export const studioPowers = () => invoke('studio_powers')
export const voiceStudioStatus = () => invoke('voicestudio_status')
export const voiceStudioSpeak = (text) => invoke('voicestudio_speak', { text })
// A helper's notice (a key was missing): { at, kind, file, reason, settings }
export const onNotice = on('castle-notice')
// The tray's Wake companion.
export const onWakeRequest = (handler) => listen('castle-wake', () => handler())

// DEV: a session per project folder, up to 8 tabs (slot 0-7). Each state call answers
// { tabs: [{ slot, folder, name, running, status: { ready, message, fix } }], projects: [{ folder, name }], memoryFolder }.
// A refusal comes back as a plain message string.
export const devState = () => invoke('dev_state')
export const devOpen = (folder) => invoke('dev_open', { folder })
export const devCloseTab = (slot) => invoke('dev_close_tab', { slot })
export const devForget = (folder) => invoke('dev_forget', { folder })
export const devWake = (slot, cols, rows) => invoke('dev_wake', { slot, cols, rows })
export const devWrite = (slot, data) => invoke('dev_write', { slot, data })
export const devResize = (slot, cols, rows) => invoke('dev_resize', { slot, cols, rows })
// the tab's screen so far, to draw the Log again
export const devSnapshot = (slot) => invoke('dev_snapshot', { slot })
// -> { entries, reset, status }, the same as chatPoll
export const devChatPoll = (slot) => invoke('dev_chat_poll', { slot })
// true when the project has changes not yet committed
export const devGitDirty = (slot) => invoke('dev_git_dirty', { slot })
// -> { available, message, plans: [{ id, title, phase, status, items: [{ ordinal, kind, text, state }] }] }
export const devPlanItems = (slot) => invoke('dev_plan_items', { slot })
export const devPlanBody = (slot, id) => invoke('dev_plan_body', { slot, id })
export const onDevOutput = (slot, handler) => listen(`dev-output-${slot}`, (event) => handler(event.payload))
export const onDevExit = (slot, handler) => listen(`dev-exit-${slot}`, (event) => handler(event.payload))
// Which DEV tabs' chats are showing (their chat is read about once a second only then).
const devShownSlots = new Set()
export const devChatShown = (slot, on) => (on ? devShownSlots.add(slot) : devShownSlots.delete(slot))
