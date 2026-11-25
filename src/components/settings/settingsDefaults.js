// Every plain setting's default, in one place, the same as the core's Settings::default
// (src-tauri/src/settings.rs; settingsDefaults.test.js holds the two together). A missing value
// always means the default: read a setting with valueOf, never with an inline `?? x` or `!== false`.

export const SETTINGS_DEFAULTS = {
  cli: 'claude',
  automationsPaused: false,
  automationsCommands: [],
  startWithWindows: false,
  startSessionOnLaunch: true,
  minimizeToTaskbar: false,
  alertSound: false,
  alertSoundName: 'glass',
  alertNotify: true,
  alertWhen: 'all',
  alertReactions: false,
  roomPicturesOn: false,
  roomPicturesPerDay: 1,
  roomPictureQuality: 'low',
  roomVoicesOn: false,
  roomVoicesPerDay: 2,
  roomShelfOn: false,
  dailyMoodOn: true,
  keeperSpeaks: true,
  displayClock: false,
  clockFormat: '24h',
  voicestudioOn: false,
  livingWallpaper: true,
  wallpaperSound: true,
  backupAfterDiary: false,
  linkOn: true,
  roomMayPass: false,
  onlyThisCompanion: false,
  diaryLook: 'list',
  mainStart: 'rested',
  shareFallback: true,
  linkOpen: 'castle',
  browserRemember: true,
  browserPrivateRemember: false,
  browserProfileRead: true,
  browserNewTab: 'shortcuts',
  browserOpens: 'window',
  // the download folder's default (null: the system's Downloads folder) is not listed: valueOf gives
  // undefined for it either way, and the core-defaults test reads only literal values
  downloadsSort: true,
  emotionRoomVoice: true,
  profileLook: 'cards',
}

/** A setting's value, or its default when it is missing (null or undefined). */
export function valueOf(settings, key) {
  const value = settings?.[key]
  return value === undefined || value === null ? SETTINGS_DEFAULTS[key] : value
}

/** True when a setting differs from its default (a row may then offer Reset). */
export function differsFromDefault(settings, key) {
  if (!(key in SETTINGS_DEFAULTS)) return false
  const value = valueOf(settings, key), fallback = SETTINGS_DEFAULTS[key]
  return Array.isArray(fallback) ? !Array.isArray(value) || value.length !== fallback.length || value.some((v, i) => v !== fallback[i]) : value !== fallback
}
