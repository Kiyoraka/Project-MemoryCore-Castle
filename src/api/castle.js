// The only file that talks to Castle's Rust core.
import { Channel, invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { typeLine } from '../components/typeline.js'
import { devSlotOf, pasted } from '../views/dev/devtabs.js'

export const hideCastle = () => invoke('hide_castle')

// Settings. -> { settings, companionLabel, cliLabel, warning, startupNote }
export const getSettings = () => invoke('get_settings')
// The chat alert sounds: the list (Windows', Castle's pack, then the member's own .wav files), a preview
// and the folder where their own go. -> [{ id, label, builtIn }] / bool / the folder's path
// Every kind of alert (CHAT, HOME replies, MARKET rounds, pictures...) with the rule now in force, the
// room it opens, and which room is on screen (so an alert stays quiet while its room is showing).
export const alertSources = () => invoke('alert_sources')
export const alertActiveRoom = (room) => invoke('alert_active_room', { room })
export const alertSounds = () => invoke('alert_sounds')
export const alertSoundPreview = (name) => invoke('alert_sound_preview', { name })
export const alertSoundsFolder = () => invoke('alert_sounds_folder')
export const castleWindowLayout = (layout) => invoke('castle_window_layout', { layout })
// the Mac's Window menu (or its full screen) changed the window size
export const onWindowLayoutChanged = (handler) => listen('window-layout-changed', () => handler())
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
// The member's Claude Code voice (/voice), for Chat's mic. -> { enabled, mode: 'hold' | 'tap' }
export const claudeVoice = () => invoke('claude_voice')
export const providersList = () => invoke('providers_list')
export const imageJobs = () => invoke('image_jobs')
export const imageRetry = (id) => invoke('image_retry', { id })
// Takes finished requests off the list: the one with this id, or all of them with no id.
export const imageDismiss = (id = null) => invoke('image_dismiss', { id })
export const imageCancel = (id) => invoke('image_cancel', { id })
// `label`: what the picture is for, shown in the Image requests list ("Try-on: Winter coat + Twin tails").
// `look` names an APPEARANCE drawing by its parts (kind, outfit, hair, changes, quality); castle-look.mjs writes its prompt
export const imageRequest = (brief, output = null, references = [], room = 'studio', label = null, look = null) => invoke('image_request', { brief, output, references, room, label, look })
export const providerSwitch = (target) => invoke('provider_switch', { target })
// The home chat starts fresh: the talk kept in Chat and carried word for word, never compacted
export const freshStart = () => invoke('fresh_start')
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
// whether the room's Claude Code session still holds the sent words in its input box (messages.rs)
export const chatPromptHolds = (room, text) => invoke('chat_prompt_holds', { room, text })
// `greet`: only the Wake button asks for it - Castle then opens the conversation so the companion greets.
// `fresh`: the member pressed "Start fresh" - a new talk whatever the start setting says
export const terminalStart = (cols, rows, room, greet = false, fresh = false) => invoke('terminal_start', { cols, rows, room, greet, fresh })
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
// A film or a long sound Chat showed, allowed on the asset protocol so the screen can stream it
// (convertFileSrc) instead of reading it whole. A DEV tab asks its own door.
export const chatMediaStream = (path, room) => {
  const slot = devSlotOf(room)
  if (slot === null) return invoke('chat_media_stream', { path, room })
  devMediaSlots.set(path, slot)
  return invoke('dev_chat_media_stream', { slot, path })
}
// The plan Claude Code is asking about, read from its plan file (a plain .md in the user's own
// plans folder) while the session record has not carried it yet.
export const chatPlanFile = (path) => invoke('chat_plan_file', { path })
// Pictures the member dropped or pasted; returns the ones Chat may show.
export const chatAttach = (paths) => invoke('chat_attach', { paths })
// The markup sheet's finished PNG (a data address) saved beside `original` as its marked copy;
// `previous` is the copy shown for it now, written over when Castle made it. -> the copy's path
export const chatMarkupSave = (original, picture, previous = null) => invoke('chat_markup_save', { original, picture, previous })

// Which MemoryCore features the member's folder holds. -> { diary, projects, library, saveMemory, ... }
export const memoryFeatures = () => invoke('memory_features')
// Everything HOME shows. today = the member's local date (YYYY-MM-DD). -> { features, reminders, projects, diary, growth } or null
export const memoryHome = (today) => invoke('memory_home', { today })
// A room's shelves (room: diary | projects | library) -> [{ name, pages: [{ title, file, date }] }]
export const memoryShelves = (room) => invoke('memory_shelves', { room })
// LIBRARY: the member's own books (memory database) and the Spiral Library (the Portfolio, read-only)
export const libraryBooks = () => invoke('library_books')
export const libraryRead = (id) => invoke('library_read', { id })
export const libraryDelete = (id) => invoke('library_delete', { id })
export const spiralRoom = () => invoke('spiral_room')
export const spiralRead = (reference) => invoke('spiral_read', { reference })
// Keep a Spiral book read whole in My books (again = update) -> My books; and ref -> edition kept
export const spiralKeep = (reference) => invoke('spiral_keep', { reference })
export const spiralKept = () => invoke('spiral_kept')
// One memory page's markdown (only .md inside the memory folder).
export const memoryRead = (file) => invoke('memory_read', { file })

// The desktop.
// the graphics-card gauges ({ gauges, on, found }) and their switch
export const gpuMeters = () => invoke('gpu_meters')
export const gpuGaugesSave = (on) => invoke('gpu_gauges_save', { on })
export const weatherGet = () => invoke('weather_get')
export const weatherSearch = (query) => invoke('weather_search', { query })
export const weatherSave = (place, units) => invoke('weather_save', { place, units })
export const weatherHere = (lat, lon) => invoke('weather_here', { lat, lon })

// The member's own ComfyUI (Settings > ComfyUI). The choices are { on, address, start, model, workflow, limitMinutes };
// saved only by comfySave, which checks them (and the workflow file) and says why in plain words when it cannot.
export const comfyGet = () => invoke('comfy_get')
export const comfySave = (on, address, start, model, workflow, limitMinutes) => invoke('comfy_save', { on, address, start, model, workflow, limitMinutes })
// -> { up, address, checkpoints, startedByCastle, message, workflowProblem }. comfyCheck only asks; comfyStart
// runs the start file first when nothing answers and waits for ComfyUI to come up.
export const comfyCheck = () => invoke('comfy_check')
export const comfyStart = () => invoke('comfy_start')
// A file picker: kind 'workflow' (a .json) or 'start' (.bat, .cmd, .sh); null when the member closes it.
export const comfyChooseFile = (kind) => invoke('comfy_choose_file', { kind })
export const weatherRead = (force = false) => invoke('weather_read', { force })

// DISKS, HOME's drive-space card. diskDrives -> [{ mount, label, total, free (bytes), removable, media }]; read only.
export const diskDrives = () => invoke('disk_drives')
// SWEEP, HOME's cleaner. sweepStatus -> { settings: { look, recycleBin }, lastSwept (unix s or null), deep (Deep clean offered) };
// sweepMeasure -> [{ id, label, bytes, files, note }] (nothing deleted; the groups offered on this computer);
// sweepClean(groups) -> [{ id, freed, files, skipped }]; sweepDeep -> { id, freed, files, skipped } or null when refused.
export const sweepStatus = () => invoke('sweep_status')
export const sweepMeasure = () => invoke('sweep_measure')
export const sweepClean = (groups) => invoke('sweep_clean', { groups })
export const sweepDeep = () => invoke('sweep_deep')
export const sweepSettingsSave = (look, recycleBin) => invoke('sweep_settings_save', { look, recycleBin })

// The Sentinel: the member's companion keeps watch over this computer.
// -> { supported, on, installed, current, setupLine, patrol: { status, line, minutesAgo } | null,
//      patrolling, unread, open: { alarm, look, note }, watchMinutes, problem }
export const sentinelStatus = () => invoke('sentinel_status')
// Two copies of Castle on this PC: { this, installed, leftover, two } (install_check.rs)
export const installCopies = () => invoke('install_copies')
// Test my guard: the three checks, on the member's press (it takes up to three minutes).
export const sentinelTest = () => invoke('sentinel_test')
export const sentinelPatrolNow = () => invoke('sentinel_patrol_now')
export const sentinelSettingsSave = (on, model, patrolMinutes, network = null) => invoke('sentinel_settings_save', { on, model, patrolMinutes, network })
export const onSentinelChanged = (handler) => listen('sentinel-changed', () => handler())
// -> [{ id, kind, hint, title, detail, fixMove, fixArg, state, stateWhy, firstSeen, lastSeen, undoable }]
export const sentinelFindings = (states) => invoke('sentinel_findings', { states })
// The member's own click on one finding: word is fix, accept or undo. -> the line to show
export const sentinelClick = (id, word) => invoke('sentinel_click', { id, word })
export const sentinelLog = () => invoke('sentinel_log')
export const sentinelAlarmsSeen = () => invoke('sentinel_alarms_seen')
// -> { marks: [{ host, verdict, reason, by, at }], patterns: [{ id, kind, value, verdict, why, learnedFrom, active }] }
export const sentinelSites = () => invoke('sentinel_sites')
export const sentinelSiteSafe = (host) => invoke('sentinel_site_safe', { host })
export const sentinelPatternOff = (id) => invoke('sentinel_pattern_off', { id })
// -> { supported, installLine, uninstallLine, installed: { installed, current, line }, recommended, cliLabel }
export const sentinelSetup = () => invoke('sentinel_setup')
// while a sweep runs: { group, freed } (bytes freed so far in all)
export const onSweepProgress = (handler) => listen('castle://sweep', (event) => handler(event.payload))
export const systemMeters = () => invoke('system_meters')
// The gauges' gear: watch CPU and RAM, tidy RAM, the last trips, close an ordinary app.
export const meterWatchGet = () => invoke('meter_watch_get')
export const meterWatchSave = (on, cpu, memory) => invoke('meter_watch_save', { on, cpu, memory })
export const meterTrimNow = () => invoke('meter_trim_now')
export const meterTrips = () => invoke('meter_trips')
export const meterClose = (pid, name) => invoke('meter_close', { pid, name })
export const onMeterTrip = (handler) => listen('castle://meter-trip', (event) => handler(event.payload))
// HOME's credit widgets: OpenAI, Seedance, Suno and ElevenLabs.
export const creditsRead = (month, monthStart) => invoke('credits_read', { month, monthStart })
export const creditsSet = (vendor, usd, month) => invoke('credits_set', { vendor, usd, month })
export const creditsBudget = (usd) => invoke('credits_budget', { usd })
export const creditsSignIn = (vendor) => invoke('credits_sign_in', { vendor })
export const creditsReadNow = (vendor) => invoke('credits_read_now', { vendor })
export const creditsStopReading = (vendor) => invoke('credits_stop_reading', { vendor })
export const onCreditsChanged = (handler) => listen('castle://credits-changed', () => handler())
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
// STUDIO's workflows: Castle's own (read-only) and the member's, each { id, source, listing, workflow, error }.
export const workflowList = () => invoke('workflow_list')
// -> [{ block, message }] (empty when the workflow is fine)
export const workflowCheck = (workflow) => invoke('workflow_check', { workflow })
// replace: one of the member's own ids to overwrite (Edit); null makes a new one. -> the id, or rejects with problems
export const workflowSave = (workflow, replace = null) => invoke('workflow_save', { workflow, replace })
export const workflowDuplicate = (id) => invoke('workflow_duplicate', { id })
export const workflowDelete = (id) => invoke('workflow_delete', { id })
export const workflowFolder = () => invoke('workflow_folder')
// both open Windows' own dialog; null when it is closed
export const workflowExport = (id) => invoke('workflow_export', { id })
export const workflowImport = () => invoke('workflow_import')
// values: { <block id>: { text, name, picture } } -> the image job (it shows in Image requests like every STUDIO picture)
export const workflowRun = (id, values) => invoke('workflow_run', { id, values })
// "Write prompt from picture": the picture's prompt in words (the key or Codex, as pictures are drawn)
export const pictureDescribe = (path) => invoke('picture_describe', { path })
// a workflow's upload tile: the picker, then the picture kept inside the memory (shown and followed from there); null when closed
export const workflowPickPicture = (title) => invoke('workflow_pick_picture', { title })
// STUDIO's Marketplace (the Portfolio's doors, through the core with the member's sign-in; shown only
// when memberStatus().marketplace.live)
export const shopListings = (query = '', page = 0) => invoke('market_shop_listings', { query, page })
export const shopListing = (id) => invoke('market_shop_listing', { id })
export const shopCheckout = (id) => invoke('market_shop_checkout', { id })
export const shopPurchases = () => invoke('market_shop_purchases')
export const shopInstall = (purchase, listing) => invoke('market_shop_install', { purchase, listing })
// up: true (thumbs up), false (down), null (take the vote back)
export const shopVote = (listing, up) => invoke('market_shop_vote', { listing, up })
export const shopConnect = () => invoke('market_shop_connect')
export const shopMine = () => invoke('market_shop_mine')
// listing: null for a new listing, or the listing to send a new version of
export const shopPublish = (workflow, listing, samples, priceMyr, description) => invoke('market_shop_publish', { workflow, listing, samples, priceMyr, description })
export const shopWithdraw = (listing) => invoke('market_shop_withdraw', { listing })
// the town's shops ({ items, missing: true } until the Portfolio's shops door answers), one shop by its slug
export const townShops = (query = '', page = 0) => invoke('market_shop_shops', { query, page })
export const townShop = (slug) => invoke('market_shop_shop', { slugName: slug })
// the member's own shop (null when none yet); save takes { name, keeperName, greeting, keeperCard, open } and optional picture paths
export const myShop = () => invoke('market_shop_my_shop')
// the shopkeeper speaks (one short run of the member's own CLI, public facts only): explain the item, or
// answer the visitor's question. shop { name, keeperName, keeperCard, seller }; item { id, name, kind,
// description, priceMyr, votesUp, votesDown, blocks: [{ type, label }], updatedAt }
export const keeperSay = (explain, shop, item, question = null) => invoke('keeper_say', { explain, shop, item, question })
export const myShopSave = (fields, keeperPicture = null, backdrop = null) => invoke('market_shop_my_shop_save', { fields, keeperPicture, backdrop })
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
// the look in words and its art style, kept in the memory database
export const appearanceSaveLook = (words, artStyle) => invoke('appearance_save_look', { words, artStyle })
// new name or words for one outfit or hairstyle
export const appearanceSaveItem = (kind, id, name, words) => invoke('appearance_save_item', { kind, id, name, words })

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
export const memberSignInCancel = () => invoke('member_sign_in_cancel')
export const memberStatus = () => invoke('member_status')
export const memberSignOut = () => invoke('member_sign_out')
export const roomCompanionIn = (on) => invoke('room_companion_in', { on })

// The MemoryCore Foundation room (keys stay in the core). Refusals come back as { kind, message }.
export const roomMe = (who) => invoke('room_me', { who })
export const roomLines = (after, wait) => invoke('room_lines', { after, wait })
export const roomPresence = () => invoke('room_presence')
export const roomPost = (text, who, replyTo = null) => invoke('room_post', { text, who, replyTo })
// the member's reaction on a line, added (on) or taken back: { id, reactions }
export const roomReact = (id, emoji, on) => invoke('room_react', { id, emoji, on })
export const roomFile = (id) => invoke('room_file', { id })
export const roomPictureSave = (address, name) => invoke('room_picture_save', { address, name })
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
export const roomReconnect = () => invoke('room_reconnect')
// the companion's room turns and its public card
export const roomLog = () => invoke('room_log')
export const roomPicturesStatus = () => invoke('room_pictures_status')
export const roomVoicesStatus = () => invoke('room_voices_status')
export const roomShelf = () => invoke('room_shelf')
export const moodToday = () => invoke('mood_today')
export const moodHistory = (days) => invoke('mood_history', { days })
export const roomShelfRefresh = () => invoke('room_shelf_refresh')
export const roomShelfRemove = (index) => invoke('room_shelf_remove', { index })
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
//   version, downloadBytes, partialBytes (an interrupted download's size on disk, 0 if none), keepRunning }; dbMemory -> { name, reachable, movedIn, canRestore };
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
// the memory's skills -> [{ plugin, name, description, stubbed, fullBytes, stubBytes, flows }]; a
// stubbed skill keeps a small SKILL.md on disk and its instructions are loaded from the database
// when it fires (by flow, when it has flows)
export const dbSkills = () => invoke('db_skills')
export const dbSkillStub = (plugin, name, on) => invoke('db_skill_stub', { plugin, name, on })
// every skill at once: all from the database (on) or all whole files again
export const dbSkillStubAll = (on) => invoke('db_skill_stub_all', { on })
// The emotion engine (PROFILE). A trait: { number, name, line, body, audience, origin, why, status, updatedAt }.
export const dbTraits = () => invoke('db_traits')
// draft = { name, line, body, audience, why }; number given = new words for that trait, else a new active one
export const dbTraitSave = (draft, number = null, origin = null) => invoke('db_trait_save', { draft, number, origin })
export const dbTraitActivate = (number) => invoke('db_trait_activate', { number })
// a trait she lives is retired; a proposal is let go
export const dbTraitRetire = (number) => invoke('db_trait_retire', { number })
// kind 'diary' (her recent diary) or 'idea' (the member's words) -> [{ name, line, body, audience, why }]; nothing is kept
export const emotionPropose = (kind, idea = null) => invoke('emotion_propose', { kind, idea })
// MY PROFILE, the member's own readings. -> { ready, timeline: [{ month, mbti, headline: [{ grp, label, score }] }],
// current: { month, mbti, enneagram, tritype, headline, entriesRead, fromDate, toDate, scannedAt, scores, notes }, waiting: ['YYYY-MM'] }
export const profileRead = () => invoke('profile_read')
// one month in full (same shape as current), or null
export const profileMonth = (month) => invoke('profile_month', { month })
// reads every finished month not read yet -> { read, skipped: ['YYYY-MM'] }
export const profileScan = () => invoke('profile_scan')
// while it reads: { month, done, total, state: reading | read | skipped, problem }
export const onProfileProgress = on('profile-progress')
// Castle just went out of sight (minus button, tray, hotkey, close button)
export const onCastleHidden = on('castle-hidden')
// the member clicked a chat notification: show the CHAT tab
export const onOpenChat = on('castle-open-chat')
// a clicked notification: Castle came forward, show this room
export const onOpenRoom = on('castle-open-room')
// "Set it up for me": install, start, create, check - then it stops and gives the move's counts
export const dbSetupAll = () => invoke('db_setup_all')
// while it runs: { step: install | start | create | check, state: running | done }
export const onDbSetup = on('castle://db-setup')
// while the engine downloads: { done, total } in bytes
export const onDbInstall = on('castle://db-install')

// The helpers' Node. nodeStatus / nodePrepare -> { state: system | own | fetching | failed | missing | unsupported,
//   path, version (system, own), done, total (fetching), reason (failed), downloadBytes (missing) }.
// nodePrepare downloads Castle's own Node when the computer has none new enough, and is a no-op otherwise.
export const nodeStatus = () => invoke('node_status')
export const nodePrepare = () => invoke('node_prepare')
// while Castle's own Node downloads: { done, total } in bytes
export const onNodeInstall = on('castle://node-install')

// STUDIO: what the companion made, and each power's state (ready | builtin | prompt).
// madeHere: STUDIO's own gallery (media-generation and music only); without it every shelf (WORLD, music)
export const studioLibrary = (madeHere = false) => invoke('studio_library', { madeHere })
// small copies of pictures (made once, kept by Castle): -> { thumbs: [{ path, thumb }], pending }
export const studioThumbs = (paths) => invoke('studio_thumbs', { paths })
export const studioPowers = () => invoke('studio_powers')
// sends one made thing to the Recycle Bin (only inside the memory's shelves, never the standard look)
export const studioDelete = (path) => invoke('studio_delete', { path })
// "codex" | "companion" | "none": how the next APPEARANCE or STUDIO picture is drawn
export const pictureWay = () => invoke('picture_way')
export const voiceStudioStatus = () => invoke('voicestudio_status')
export const voiceStudioSpeak = (text) => invoke('voicestudio_speak', { text })
// MemoryCore in your pocket, Telegram: { on, connected, hasToken, paired, pending: { chatId, name } | null, problem }
export const telegramBridgeStatus = () => invoke('telegram_bridge_status')
export const telegramBridgeApprove = () => invoke('telegram_bridge_approve')
export const telegramBridgeForget = () => invoke('telegram_bridge_forget')
// MemoryCore in your pocket, Discord: { on, connected, hasToken, paired, pending: { channelId, name } | null, problem }
export const discordBridgeStatus = () => invoke('discord_bridge_status')
export const discordBridgeApprove = () => invoke('discord_bridge_approve')
export const discordBridgeForget = () => invoke('discord_bridge_forget')
// MemoryCore in your pocket, WhatsApp: { on, connected, paired, qr: string | null, problem,
// pending / me: { chatId, name, number } | null, guests: [{ chatId, name, number }] } (number: '+601 2345 6789' or null)
export const whatsappBridgeStatus = () => invoke('whatsapp_bridge_status')
// the chat waiting: asWho 'me' (your own number), 'guest', or 'no' (not this one)
export const whatsappBridgeApprove = (asWho = 'me') => invoke('whatsapp_bridge_approve', { asWho })
export const whatsappBridgeMakeMe = (chatId) => invoke('whatsapp_bridge_make_me', { chatId })
export const whatsappBridgeRemoveGuest = (chatId) => invoke('whatsapp_bridge_remove_guest', { chatId })
// forgets your own number; the guests stay
export const whatsappBridgeForget = () => invoke('whatsapp_bridge_forget')
// pairs the Castle with a new WhatsApp number; you and every guest stay approved
export const whatsappBridgeUnpair = () => invoke('whatsapp_bridge_unpair')
// The member's reactions from the phone on the companion's replies: [{ platform, words, emoji, at }]
export const phoneReactions = () => invoke('phone_reactions')
// One just made on the phone (an empty emoji: taken back)
export const onPhoneReaction = on('castle://phone-reaction')
// A helper's notice (a key was missing): { at, kind, file, reason, settings }
export const onNotice = on('castle-notice')
// The tray's Wake companion.
export const onWakeRequest = (handler) => listen('castle-wake', () => handler())

// DEV: projects on a list, and up to 8 sessions in all (slot 0-7), several per project if wanted.
// Each state call answers { tabs: [{ slot, folder, name, number, running, status: { ready, message, fix } }],
// projects: [{ folder, name, missing }], memoryFolder }.
// A refusal comes back as a plain message string.
export const devState = () => invoke('dev_state')
export const devOpen = (folder) => invoke('dev_open', { folder })
export const devCloseTab = (slot) => invoke('dev_close_tab', { slot })
// the live developer servers -> [{ port, pid, program, folder, slot, startedBy }] (read only, nothing is ever stopped)
export const devServers = () => invoke('dev_servers')
// one more session in a project (the first free tab), and taking a project off the list (its sessions end first)
export const devNewSession = (folder) => invoke('dev_new_session', { folder })
export const devRemoveProject = (folder) => invoke('dev_remove_project', { folder })
export const devWake = (slot, cols, rows) => invoke('dev_wake', { slot, cols, rows })
export const devWrite = (slot, data) => invoke('dev_write', { slot, data })
// DEV > Phone: Mobile setup for a tab's project, and a phone made from an installed image
export const mobileCheck = (slot) => invoke('mobile_check', { slot })
export const mobileCreatePhone = (name, image) => invoke('mobile_create_phone', { name, image })
// the docked phone: the phones here and the emulators running that this Castle did not start -> { phones, elsewhere }
export const phoneList = () => invoke('phone_list')
// start the tab's phone with no window (booting goes on in the background) -> { state, avd, serial, note }
export const phoneStart = (slot, avd) => invoke('phone_start', { slot, avd })
// where the tab's phone stands: state 'off' | 'starting' | 'booting' | 'ready' | 'failed'
export const phoneState = (slot) => invoke('phone_state', { slot })
// stop the tab's phone (only one this Castle started)
export const phoneStop = (slot) => invoke('phone_stop', { slot })
// stream the phone's screen: onFrame(ArrayBuffer) gets width and height (u32 LE) then the RGBA bytes;
// onError(text) when the stream breaks for a reason
export function phoneScreen(slot, width, onFrame, onError = () => {}) {
  const frames = new Channel()
  frames.onmessage = (message) => {
    if (message instanceof ArrayBuffer) onFrame(message)
    else if (message && message.error) onError(message.error)
  }
  return invoke('phone_screen', { slot, width, frames })
}
export const phoneScreenEnd = (slot) => invoke('phone_screen_end', { slot })
// one finger at a point on the phone's own screen, down or up
export const phoneTouch = (slot, x, y, down) => invoke('phone_touch', { slot, x, y, down })
// one of the phone's buttons ('back', 'home', 'recent', 'enter', 'backspace' ...) or typed text
export const phoneKey = (slot, button = null, text = null) => invoke('phone_key', { slot, button, text })
export const phoneRotate = (slot) => invoke('phone_rotate', { slot })
// Run on the phone: ios = the iPhone preview of a Flutter app -> { running, kind, target, preview, tail }
export const phoneRun = (slot, ios = false) => invoke('phone_run', { slot, ios })
export const phoneReload = (slot) => invoke('phone_reload', { slot })
export const phoneRestart = (slot) => invoke('phone_restart', { slot })
export const phoneRunStop = (slot) => invoke('phone_run_stop', { slot })
export const phoneRunInfo = (slot) => invoke('phone_run_info', { slot })
// the run's text as it comes, and its end (exit code)
export const onPhoneOutput = (slot) => (handler) => listen(`phone-output-${slot}`, (e) => handler(e.payload))
export const onPhoneExit = (slot) => (handler) => listen(`phone-exit-${slot}`, (e) => handler(e.payload))
export const devResize = (slot, cols, rows) => invoke('dev_resize', { slot, cols, rows })
// the tab's screen so far, to draw the Log again
export const devSnapshot = (slot) => invoke('dev_snapshot', { slot })
// -> { entries, reset, status }, the same as chatPoll
export const devChatPoll = (slot) => invoke('dev_chat_poll', { slot })

// A DEV project's own database (the Database panel). `target` = { slot, side: 'local' | 'production',
// database }; the login stays in the core and never reaches the page.
// -> { project, local, production }: each { host, port, database, user, from, hasPassword, ca, blocked }
export const projectDbConnection = (slot) => invoke('projectdb_connection', { slot })
// form = { host, port, database, user, password?: string (undefined keeps it, '' removes it), caPath?: string }
export const projectDbSave = (slot, side, form) => invoke('projectdb_save', { slot, side, form })
export const projectDbForget = (slot, side) => invoke('projectdb_forget', { slot, side })
export const projectDbTest = (target) => invoke('projectdb_test', { target })
export const projectDbPickCa = () => invoke('projectdb_pick_ca')
export const projectDbDatabases = (target) => invoke('projectdb_databases', { target })
export const projectDbTables = (target) => invoke('projectdb_tables', { target })
export const projectDbTableRows = (target) => invoke('projectdb_table_rows', { target })
export const projectDbColumns = (target, table) => invoke('projectdb_columns', { target, table })
// -> { columns, rows, truncated, affected, ms }
export const projectDbLook = (target, sqlText) => invoke('projectdb_look', { target, sqlText })
// -> { id, affected, ms }: id 0 = final; otherwise held for Keep / Undo (two minutes)
export const projectDbChange = (target, sqlText, confirmed = false) => invoke('projectdb_change', { target, sqlText, confirmed })
export const projectDbFinish = (id, keep) => invoke('projectdb_finish', { id, keep })
export const projectDbExport = (rows, name) => invoke('projectdb_export', { rows, name })
export const projectDbDump = (target, table = null) => invoke('projectdb_dump', { target, table })
export const projectDbImport = (target, table, unlocked) => invoke('projectdb_import', { target, table, unlocked })
export const projectDbSaved = (slot) => invoke('projectdb_saved', { slot })
export const projectDbSavedSet = (slot, list) => invoke('projectdb_saved_set', { slot, list })
// true when the project has changes not yet committed
export const devGitDirty = (slot) => invoke('dev_git_dirty', { slot })
// -> { available, message, plans: [{ id, title, phase, status, items: [{ ordinal, kind, text, state }] }] }
export const devPlanItems = (slot) => invoke('dev_plan_items', { slot })
export const devPlanBody = (slot, id) => invoke('dev_plan_body', { slot, id })
export const onDevOutput = (slot, handler) => listen(`dev-output-${slot}`, (event) => handler(event.payload))
export const onDevExit = (slot, handler) => listen(`dev-exit-${slot}`, (event) => handler(event.payload))
// A session that keeps filling up (unstick.rs): { slot (null = the home chat), line ('' clears it) }
export const onSessionFull = (handler) => listen('session-full', (event) => handler(event.payload))
// Which DEV tabs' chats are showing (their chat is read about once a second only then).
const devShownSlots = new Set()
export const devChatShown = (slot, on) => (on ? devShownSlots.add(slot) : devShownSlots.delete(slot))

// The MARKET room: paper money on public prices. Nothing real is ever ordered.
export const marketState = (symbol) => invoke('market_state', { symbol })
export const marketSymbols = () => invoke('market_symbols')
export const marketCandles = (symbol, interval, limit) => invoke('market_candles', { symbol, interval, limit })
export const marketCandlesBefore = (symbol, interval, limit, beforeMs) => invoke('market_candles_before', { symbol, interval, limit, beforeMs })
export const marketPreview = (order) => invoke('market_preview', { order })
export const marketPlace = (order) => invoke('market_place', { order })
export const marketCancel = (orderId) => invoke('market_cancel', { orderId })
export const marketClose = (positionId) => invoke('market_close', { positionId })
export const marketExits = (positionId, tp, sl) => invoke('market_exits', { positionId, tp, sl })
export const marketMargin = (positionId, amount) => invoke('market_margin', { positionId, amount })
export const marketLeverage = (symbol, marginMode, leverage) => invoke('market_leverage', { symbol, marginMode, leverage })
export const marketNewRun = (startBalance) => invoke('market_new_run', { startBalance })
export const marketCloseRun = () => invoke('market_close_run')
export const marketRuns = () => invoke('market_runs')
export const marketHistory = (kind, runId, page) => invoke('market_history', { kind, runId, page })
export const marketFees = (taker, maker) => invoke('market_fees', { taker, maker })
export const onMarketChanged = (handler) => listen('castle://market-changed', () => handler())
// The companion at the desk.
const tzOffsetMinutes = () => -new Date().getTimezoneOffset()
export const marketTurn = (message) => invoke('market_turn', { message, tzOffsetMinutes: tzOffsetMinutes() })
export const marketAutoGet = () => invoke('market_auto_get', { tzOffsetMinutes: tzOffsetMinutes() })
export const marketAutoSave = (s) => invoke('market_auto_save', { ...s, tzOffsetMinutes: tzOffsetMinutes() })
export const marketStop = () => invoke('market_stop')
export const marketTurns = (runId, page) => invoke('market_turns', { runId, page })
export const marketLessons = () => invoke('market_lessons')
export const marketLessonDelete = (lessonId) => invoke('market_lesson_delete', { lessonId })

// PROJECTS: where the project behind a reader page (db/project/<id>.md) is, and moving it.
// -> { name, shelf: active|paused|archived|deleted, note, daysLeft, parent, kids, kidsBeside,
//      canHaveKids, tasks: [{ id, title, done, due }], done, total }
export const projectState = (file) => invoke('project_state', { file })
// withKids: the sub-projects on the same shelf move with it
export const projectShelve = (file, to, note = null, withKids = false) => invoke('project_shelve', { file, to, note, withKids })
// a project's tasks (CASTLE-6)
export const projectTaskAdd = (file, title, due = null) => invoke('project_task_add', { file, title, due })
export const projectTaskDone = (id, done) => invoke('project_task_done', { id, done })
export const projectTaskRemove = (id) => invoke('project_task_remove', { id })
export const projectTaskOrder = (file, ids) => invoke('project_task_order', { file, ids })
// the one-time split of grouped names ("Castle · X") into sub-projects
// -> [{ parent, parentExists, items: [{ id, name, newName }] }]
export const projectSplitProposals = () => invoke('project_split_proposals')
export const projectSplitApply = (ids) => invoke('project_split_apply', { ids })

// Automations phase 1: schedules and isolated briefing jobs.
// Calendar phase 1: what has a date, read where it lives (read only).
export const calendarRange = (from, to) => invoke('calendar_range', { from, to })
// Phase 2: the member's own events.
export const calendarEventGet = (id) => invoke('calendar_event_get', { id })
export const calendarEventSave = (event) => invoke('calendar_event_save', { event })
export const calendarEventDelete = (id) => invoke('calendar_event_delete', { id })
export const calendarEventMove = (id, occurrence, newStartsAt) => invoke('calendar_event_move', { id, occurrence, newStartsAt })
export const calendarEventSkip = (id, occurrence, undo = false) => invoke('calendar_event_skip', { id, occurrence, undo })
export const calendarQuickaddPreview = (text) => invoke('calendar_quickadd_preview', { text })
// Phase 4: the companion's suggestions.
// Phase 7: calendars the member subscribes to (holidays); fetched once a day, only shown.
export const calendarSubs = () => invoke('calendar_subs')
export const calendarSubAdd = ({ preset = null, name = null, url = null }) => invoke('calendar_sub_add', { preset, name, url })
export const calendarSubSet = (id, enabled, region) => invoke('calendar_sub_set', { id, enabled, region })
export const calendarSubRemove = (id) => invoke('calendar_sub_remove', { id })
export const calendarSubRefresh = (id) => invoke('calendar_sub_refresh', { id })
// Phase 6: calendar files (.ics). The preview reads a file the member chooses and writes nothing.
export const calendarIcsPreview = () => invoke('calendar_ics_preview')
export const calendarIcsImport = (token) => invoke('calendar_ics_import', { token })
export const calendarIcsDiscard = () => invoke('calendar_ics_discard')
export const calendarIcsExport = (includeDated) => invoke('calendar_ics_export', { includeDated })
// Phase 5: an event that starts an automation.
export const calendarLinkJobs = () => invoke('calendar_link_jobs')
export const calendarProposals = () => invoke('calendar_proposals')
export const calendarProposalDecide = (id, approve, event = null) => invoke('calendar_proposal_decide', { id, approve, event })
// FINANCE: the receipts the companion proposes and the member keeps, on this computer only.
export const financeMonth = (month = null) => invoke('finance_month', { month })
export const financeDecide = (id, keep, edits = null) => invoke('finance_decide', { id, keep, edits })
export const financeExport = (month = null) => invoke('finance_export', { month })
export const financeDrop = (paths) => invoke('finance_drop', { paths })
// Phase 3: reminders.
export const calendarHome = () => invoke('calendar_home')
export const calendarNoticeDismiss = (id = null) => invoke('calendar_notice_dismiss', { id })
export const onCalendar = (fn) => listen('castle://calendar', fn)
export const automationList = () => invoke('automation_list')
export const automationSave = (job) => invoke('automation_save', { job })
export const automationRun = (id, test = false) => invoke('automation_run', { id, test })
export const automationHistory = (id) => invoke('automation_history', { id })
export const automationArchive = (id) => invoke('automation_archive', { id })
export const automationPause = (paused) => invoke('automation_pause', { paused })

export const onAutomations = (fn) => listen('castle://automations', fn)

export const automationProgramPick = () => invoke('automation_program_pick')
export const automationProgramsSave = (programs) => invoke('automation_programs_save', { programs })

export const automationHookReset = (id) => invoke('automation_hook_reset', { id })
export const automationMessageMatch = (trigger, text) => invoke('automation_message_match', {trigger, text})

export const automationDescribe = text => invoke('automation_describe', { text })

export const automationHealth = () => invoke('automation_health')

// The BROWSER window (CASTLE-12). A chat's link goes where Settings > Rooms > Browser says:
// -> 'castle' | 'system' | 'ask' (the chat then shows its small menu).
export const linkOpen = (url) => invoke('link_open', { url })
// `priv`: a private tab, which keeps nothing
export const browserOpen = (url = null, priv = false) => invoke('browser_open', { url, private: priv })
export const browserState = () => invoke('browser_state')
export const browserSwitch = (id) => invoke('browser_switch', { id })
export const browserClose = (id) => invoke('browser_close', { id })
// `id`: a pane's own tab (its toolbar); null for the tab in front
export const browserGo = (input, id = null) => invoke('browser_go', { input, id })
export const browserStep = (step, id = null) => invoke('browser_step', { step, id })
export const browserOutside = (url) => invoke('browser_outside', { url })
// The browser's first-open welcome (browser_welcome.rs): -> { answers, chrome, profiles: [{ folder, name }], companion }
export const browserWelcome = () => invoke('browser_welcome')
export const browserWelcomeAnswer = (step, answer) => invoke('browser_welcome_answer', { step, answer })
export const browserWelcomeReopen = () => invoke('browser_welcome_reopen')
export const browserImportHistory = (profile) => invoke('browser_import_history', { profile })
export const browserImportBookmarks = (profile) => invoke('browser_import_bookmarks', { profile })
export const browserPasswordGuide = () => invoke('browser_password_guide')
// Castle's own picker; -> { sealed, skipped, shredded } (counts only) or null when closed
export const browserImportPasswords = (guided) => invoke('browser_import_passwords', { guided })
// What came over from Chrome, in use (browser_library.rs); a password never comes back to a page
export const browserNewTab = () => invoke('browser_new_tab')
export const browserIsBookmarked = (url) => invoke('browser_is_bookmarked', { url })
export const browserBookmarkToggle = (url, title) => invoke('browser_bookmark_toggle', { url, title })
export const browserLogins = (tab = null) => invoke('browser_logins', { tab })
export const browserFill = (id, tab = null) => invoke('browser_fill', { id, tab })
export const vaultList = () => invoke('vault_list')
export const vaultForget = (id = null) => invoke('vault_forget', { id })
// The browser's Passwords sheet: seal a login typed by hand, copy a password for 30 seconds, import Chrome's export
export const vaultAdd = (url, username, password) => invoke('vault_add', { url, username, password })
export const vaultCopy = (id) => invoke('vault_copy', { id })
export const vaultImport = () => invoke('vault_import')
// The bookmarks bar: every bookmark by folder; Chrome's brought at a press -> { added, profiles }
export const bookmarksAll = () => invoke('bookmarks_all')
// Read this: the page to the member's companion at HOME (kind: summary | safety | form); Open HOME
export const browserRead = (id, kind) => invoke('browser_read', { id, kind })
export const browserOpenHome = () => invoke('browser_open_home')
export const bookmarksBringChrome = (profile = null) => invoke('bookmarks_bring_chrome', { profile })
export const browserPutInChat = (url) => invoke('browser_put_in_chat', { url })
export const browserClear = () => invoke('browser_clear')
// The DANGER screen's two buttons (browser_guard.rs): back to safety, or one page of the site anyway
export const browserDangerBack = (id) => invoke('browser_danger_back', { id })
export const browserDangerOpen = (id) => invoke('browser_danger_open', { id })
// The layout: how tall the toolbar page's bar is, and whether it has something open over the pages
// Where each page goes: [{ id, x, y, w, h }] measured under its pane's toolbar; [] hides every page.
// `window` is a tab's own window ('browser-out-<n>'); left out, the browser's home
export const browserShow = (placed, window) => invoke('browser_show', window ? { placed, window } : { placed })
// A tab in a window of its own (browser_out.rs): the named home tabs (a group goes together) into a
// new window, at the cursor or the remembered place -> its label; back home -> the ids that came home
export const browserPopOut = (ids, atCursor = false) => invoke('browser_pop_out', { ids, atCursor: !!atCursor })
export const browserPopIn = (window) => invoke('browser_pop_in', { window })
// Its x: its tabs close for good, then the window
export const browserOutClose = (window) => invoke('browser_out_close', { window })
export const browserOutFullscreen = (window, on) => invoke('browser_out_fullscreen', { window, on: !!on })
export const browserOutFocus = (window) => invoke('browser_out_focus', { window })
// The pages that came home from a window of their own: [id, ...]
export const onBrowserPoppedIn = (handler) => listen('browser-popped-in', (event) => handler(event.payload))
// Browser opens changed: every page closes (sign-ins kept) and opens in its new home next time
export const browserCloseAll = () => invoke('browser_close_all')
export const onBrowserShowRoom = (handler) => listen('browser-show-room', () => handler())
// The address bar's suggestions (browser_suggest.rs): -> [{ kind: 'search'|'page'|'bookmark', text, url, sub }]
export const browserSuggest = (input) => invoke('browser_suggest', { input })
export const browserForgetSearch = (term) => invoke('browser_forget_search', { term })
// Pages side by side (browser_tabs.rs): beside the page in front, out of the group, the widths, Phone view
export const browserBeside = (id) => invoke('browser_beside', { id })
export const browserUnpane = (id) => invoke('browser_unpane', { id })
export const browserWidths = (widths) => invoke('browser_widths', { widths })
export const browserPhoneView = (on) => invoke('browser_phone_view', { on })
// The translator (browser_translate.rs), through the member's own Claude Code
export const browserTranslate = (id, lang) => invoke('browser_translate', { id, lang })
export const browserTranslateOff = (id) => invoke('browser_translate_off', { id })
export const browserTranslatePictures = (id, lang) => invoke('browser_translate_pictures', { id, lang })
// -> { languages: [[code, name]], language, sites, canTranslate, canKeepSites, bubbleWay }
export const browserTranslateSettings = () => invoke('browser_translate_settings')
export const browserTranslateLanguage = (code) => invoke('browser_translate_language', { code })
export const browserTranslateSite = (id, on) => invoke('browser_translate_site', { id, on })
// The downloader (downloads.rs): the list, a pasted link, and each card's buttons
export const downloadsList = () => invoke('downloads_list')
export const onDownloadsState = (handler) => listen('downloads-state', (event) => handler(event.payload))
export const downloadAdd = (url) => invoke('download_add', { url })
export const downloadPause = (id) => invoke('download_pause', { id })
export const downloadResume = (id) => invoke('download_resume', { id })
export const downloadCancel = (id) => invoke('download_cancel', { id })
export const downloadRemove = (id) => invoke('download_remove', { id })
export const downloadOpen = (id) => invoke('download_open', { id })
export const downloadShow = (id) => invoke('download_show', { id })
// Videos (downloads_video.rs): the tool -> { installed, ffmpeg }, a link's qualities -> { title, choices, pick }
export const videoToolState = () => invoke('video_tool_state')
export const videoToolGet = () => invoke('video_tool_get')
export const videoFormats = (url) => invoke('video_formats', { url })
export const videoAdd = (url, choice, title) => invoke('video_add', { url, choice, title })
export const videoFromPage = (id) => invoke('video_from_page', { id })
export const onBrowserState = (handler) => listen('browser-state', (event) => handler(event.payload))
export const onBrowserPutInChat = (handler) => listen('browser-put-in-chat', (event) => handler(event.payload))
// How much is new across the browser's tabs, read from each site's own title: -> { count, title, away }
export const onBrowserUnread = (handler) => listen('browser-unread', (event) => handler(event.payload))
