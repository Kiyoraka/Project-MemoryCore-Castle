// HOME's widgets as the member arranges them: which show, how big, and where. ONE arrangement for
// every theme: a theme's own places only decide where a widget FIRST appears; the moment it has
// appeared its place is kept (SHARED below), so changing theme or restarting never moves it. The
// member's place is a top-left corner in percent of HOME; the theme's width is kept.

export const WIDGETS = [
  { id: 'clock', name: 'Clock', about: 'The time and the date.' },
  { id: 'gauges', name: 'Gauges', about: "Your computer's CPU and memory." },
  { id: 'weather', name: 'Weather', about: 'The weather where you are, and the next days. Uses your location if you allow it, or a city you search.' },
  { id: 'disks', name: 'Disks', about: 'How much room each drive of your computer has left.' },
  { id: 'sentinel', name: 'Sentinel', about: 'Your companion keeping watch over this computer: the state, the last patrol and whether the watch is running.' },
  { id: 'automations', name: 'Automations', about: 'What your automations last reported, and whether they are paused. Shown while the AUTOMATIONS room is on.', room: 'automations' },
  { id: 'calendar', name: 'Calendar', about: 'Your event reminders (also the ones missed while Castle was closed) and the next events. Shown while the CALENDAR room is on.', room: 'calendar' },
  { id: 'sweep', name: 'Sweep', about: 'Frees space: temp files, caches and more, after a preview.' },
  { id: 'usage', name: 'Usage', about: 'How much of your Claude Code and Codex limits is used.' },
  { id: 'today', name: 'Today', about: 'Your reminders for today.' },
  { id: 'projects', name: 'Projects', about: 'Your active projects.' },
  { id: 'diary', name: 'Diary', about: 'The newest diary entry.' },
  { id: 'growth', name: 'Growth', about: 'How your memory has grown.' },
  { id: 'backup', name: 'Backup', about: 'When your memory was last backed up.' },
  { id: 'database', name: 'Database', about: 'Your memory database and its engine.' },
  { id: 'credit-openai', name: 'OpenAI', about: 'What is left on your OpenAI account, for pictures. Shown once its key is saved.', key: 'openai' },
  { id: 'credit-seedance', name: 'Seedance', about: "This month's Seedance spend on BytePlus, for videos. Shown once its key is saved.", key: 'seedance' },
  { id: 'credit-suno', name: 'Suno', about: 'Your Suno credits, for songs. Shown once its key is saved.', key: 'suno' },
  { id: 'credit-elevenlabs', name: 'ElevenLabs', about: 'The characters left on your ElevenLabs plan, for voices. Shown once its key is saved.', key: 'elevenlabs' },
]

// A credit widget shows only once its key is saved in Settings > Keys; `keys` lists the saved ones.
export function hasItsKey(id, keys = []) {
  const key = WIDGETS.find((w) => w.id === id)?.key
  return !key || keys.includes(key)
}

// A widget that belongs to a room shows only while that room is on (`rooms` is the settings' room switches).
export function roomIsOn(id, rooms = {}) {
  const room = WIDGETS.find((w) => w.id === id)?.room
  return !room || rooms[room] !== false
}

export const SIZES = { s: 0.85, m: 1, l: 1.15 }

// Widgets that stay hidden until the member switches them on (listed in `widgetsOn`).
export const OPT_IN = ['weather']

// Is this widget shown? USAGE has its own switch (it also stops Castle asking the CLIs); an opt-in
// widget is shown only once it was switched on.
export function isOn(id, settings = {}) {
  if (id === 'usage') return settings.showUsageCard !== false
  if (OPT_IN.includes(id)) return (settings.widgetsOn ?? []).includes(id)
  return !(settings.widgetsOff ?? []).includes(id)
}

// The settings change for switching a widget on or off.
export function switchPatch(id, on, settings = {}) {
  if (id === 'usage') return { showUsageCard: on }
  if (OPT_IN.includes(id)) {
    const wanted = new Set(settings.widgetsOn ?? [])
    if (on) wanted.add(id)
    else wanted.delete(id)
    return { widgetsOn: [...wanted].sort() }
  }
  const off = new Set(settings.widgetsOff ?? [])
  if (on) off.delete(id)
  else off.add(id)
  return { widgetsOff: [...off].sort() }
}

// A widget's size: 's', 'm' or 'l'.
export function sizeOf(id, settings = {}) {
  const size = settings.widgetSizes?.[id]
  return size in SIZES ? size : 'm'
}

// How large a widget is drawn: the member's own size for it, times the theme's starting scale
// ("widgetScale" in theme.json, for a theme whose design draws its widgets larger; 1 when it has
// none, and kept between 0.75 and 1.6 so no theme can make HOME unusable).
export function scaleOf(id, settings = {}, theme = null) {
  const wanted = Number(theme?.widgetScale)
  const themeScale = Number.isFinite(wanted) && wanted > 0 ? Math.min(1.6, Math.max(0.75, wanted)) : 1
  return Math.round(SIZES[sizeOf(id, settings)] * themeScale * 1000) / 1000
}

// The corner a widget grows from when it is drawn larger: the corner it is anchored to, so a widget
// placed from the right or the bottom grows inward and never off HOME.
export function originOf(place = {}) {
  return `${place.right != null && place.left == null ? 'right' : 'left'} ${place.bottom != null && place.top == null ? 'bottom' : 'top'}`
}

// The settings change for a widget's size ('m' is the default and is not kept).
export function sizePatch(id, size, settings = {}) {
  const sizes = { ...(settings.widgetSizes ?? {}) }
  if (size === 'm' || !(size in SIZES)) delete sizes[id]
  else sizes[id] = size
  return { widgetSizes: sizes }
}

// Where a widget sits: the member's corner (percent) with the theme's width and height, or the
// theme's own place untouched.
export function placeFor(themePlace = {}, memberPlace = null) {
  if (!memberPlace) return themePlace
  const place = { left: `${memberPlace.left}%`, top: `${memberPlace.top}%` }
  if (themePlace.width != null) place.width = themePlace.width
  if (themePlace.height != null) place.height = themePlace.height
  return place
}

// The key under which the one arrangement is kept in `widgetPlaces`.
export const SHARED = 'all'

// The member's place for a widget, or null while it has none. A place kept per theme by an older
// Castle still counts for that theme until the arrangement is saved once (`adoptPatch`).
export function memberPlace(settings = {}, themeId, id) {
  return settings.widgetPlaces?.[SHARED]?.[id] ?? settings.widgetPlaces?.[themeId]?.[id] ?? null
}

// The widgets (of `ids`) that have no kept place yet.
export function unplaced(settings = {}, themeId, ids = []) {
  return ids.filter((id) => !memberPlace(settings, themeId, id))
}

// The settings change that keeps places for widgets that have none: `found` maps a widget id to the
// place it has on HOME right now. Places already kept are never touched, and neither are other
// themes' old places.
export function adoptPatch(settings = {}, themeId, found = {}) {
  const wanted = Object.entries(found).filter(([id]) => !memberPlace(settings, themeId, id))
  if (!wanted.length) return null
  const all = { ...(settings.widgetPlaces ?? {}) }
  const shared = { ...(all[SHARED] ?? {}) }
  // the old per-theme places of this theme come along, so nothing the member arranged is lost
  for (const [id, place] of Object.entries(all[themeId] ?? {})) if (!shared[id]) shared[id] = place
  for (const [id, place] of wanted) shared[id] = place
  all[SHARED] = shared
  return { widgetPlaces: all }
}

const round = (n) => Math.round(n * 10) / 10

// A dropped widget's corner in percent, kept wholly on HOME: `box` is where the widget now is
// (left, top, width, height in pixels, relative to HOME), `home` is HOME's width and height.
export function dropPlace(box, home) {
  if (!home?.width || !home?.height) return { left: 0, top: 0 }
  const maxLeft = Math.max(0, home.width - box.width)
  const maxTop = Math.max(0, home.height - box.height)
  const left = Math.min(Math.max(0, box.left), maxLeft)
  const top = Math.min(Math.max(0, box.top), maxTop)
  return { left: round((left / home.width) * 100), top: round((top / home.height) * 100) }
}

// The settings change for one widget put down: it is kept for every theme. (`themeId` is only used
// to bring this theme's older places along.)
export function placePatch(settings = {}, themeId, id, place) {
  const all = { ...(settings.widgetPlaces ?? {}) }
  const shared = { ...(all[SHARED] ?? {}) }
  for (const [other, p] of Object.entries(all[themeId] ?? {})) if (!shared[other]) shared[other] = p
  shared[id] = place
  all[SHARED] = shared
  return { widgetPlaces: all }
}

// The settings change for "Reset the layout": every kept place goes, so each widget is placed
// afresh by the theme in use and then kept again.
export function resetPatch() {
  return { widgetPlaces: {} }
}

// HOME without arrange mode: a press on a widget becomes a drag only once the pointer has moved
// this far, so a press that stays put is still a click.
export const DRAG_START_PX = 6

export function dragBegins(dx, dy) {
  return Math.hypot(dx, dy) > DRAG_START_PX
}

// A press that starts on something the member works with (a button, a link, a field) never drags.
const CONTROL_TAGS = ['BUTTON', 'A', 'INPUT', 'SELECT', 'TEXTAREA', 'LABEL', 'SUMMARY']
export function startsOnControl(path = []) {
  return path.some((tag) => CONTROL_TAGS.includes(String(tag).toUpperCase()))
}
