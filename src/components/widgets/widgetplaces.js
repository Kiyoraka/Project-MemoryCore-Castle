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
  return Math.round(SIZES[sizeOf(id, settings)] * themeScaleOf(theme) * 1000) / 1000
}

// The theme's starting scale alone ("widgetScale", kept between 0.75 and 1.6; 1 when it has none).
export function themeScaleOf(theme = null) {
  const wanted = Number(theme?.widgetScale)
  return Number.isFinite(wanted) && wanted > 0 ? Math.min(1.6, Math.max(0.75, wanted)) : 1
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

// SNAP TO GRID: a widget put down lands on an even grid of GRID_PX pixels, counted from HOME's
// top-left corner, so widgets line up without pixel hunting. On unless the member turned it off.
export const GRID_PX = 16

export function snapOn(settings = {}) {
  return settings.widgetSnap !== false
}

export function snapPatch(on) {
  return { widgetSnap: !!on }
}

// A length in pixels on the grid (`grid` 0 or less leaves it as it is).
export function snapPx(n, grid = GRID_PX) {
  return grid > 0 ? Math.round(n / grid) * grid : n
}

// A dropped widget's corner in percent, kept wholly on HOME: `box` is where the widget now is
// (left, top, width, height in pixels, relative to HOME), `home` is HOME's width and height. With
// `grid`, the corner lands on the grid first (and stays on HOME).
export function dropPlace(box, home, grid = 0) {
  if (!home?.width || !home?.height) return { left: 0, top: 0 }
  const maxLeft = Math.max(0, home.width - box.width)
  const maxTop = Math.max(0, home.height - box.height)
  const left = Math.min(Math.max(0, snapPx(box.left, grid)), maxLeft)
  const top = Math.min(Math.max(0, snapPx(box.top, grid)), maxTop)
  return { left: round((left / home.width) * 100), top: round((top / home.height) * 100) }
}

// ARRANGE FOR ME: the widgets laid out on HOME so none overlaps another or the companion's panel.
// All in pixels relative to HOME. `boxes` is [{ id, width, height }] in the order they are wanted
// (the first takes the top-left free spot); `area` is { left, top, right, bottom }, the room they may
// use; `blocked` is [{ left, top, width, height }] they must keep clear of (the panel, a button).
// Each widget takes the highest free spot, then the leftmost, `gap` apart from everything. Returns
// the corners found and the ids that did not fit anywhere (`left`).
export function packBoxes(boxes = [], area, blocked = [], { gap = GRID_PX, grid = 0 } = {}) {
  const taken = blocked.map((b) => ({ left: b.left, top: b.top, width: b.width, height: b.height }))
  const up = (n) => (grid > 0 ? Math.ceil(n / grid) * grid : n)
  const clear = (a, b) => a.left >= b.left + b.width + gap || b.left >= a.left + a.width + gap || a.top >= b.top + b.height + gap || b.top >= a.top + a.height + gap
  const places = {}
  const left = []
  for (const box of boxes) {
    // the spots worth trying: the area's own edges and just past every edge already taken
    const xs = new Set([up(area.left)])
    const ys = new Set([up(area.top)])
    for (const t of taken) {
      xs.add(up(t.left + t.width + gap))
      ys.add(up(t.top + t.height + gap))
    }
    const spots = []
    for (const y of ys) for (const x of xs) spots.push({ left: x, top: y })
    spots.sort((a, b) => a.top - b.top || a.left - b.left)
    const spot = spots.find((s) => {
      const r = { left: s.left, top: s.top, width: box.width, height: box.height }
      return r.left >= area.left && r.top >= area.top && r.left + r.width <= area.right && r.top + r.height <= area.bottom && taken.every((t) => clear(r, t))
    })
    if (!spot) {
      left.push(box.id)
      continue
    }
    places[box.id] = spot
    taken.push({ ...spot, width: box.width, height: box.height })
  }
  return { places, left }
}

// ARRANGE FOR ME with the sizes in mind: `items` is [{ id, size, unit: { width, height } }], `unit`
// being the widget's box at size M, in the order the member reads them. First every widget keeps its
// size; when some do not fit, L widgets become M, then every widget S. At each step the member's
// order is tried first, then tallest, largest and widest first (which pack tighter), so a widget is
// made smaller only when no order makes room. The try that leaves out the fewest wins.
// Returns { places, sizes, left, shrunk }.
const ORDERS = [
  () => 0,
  (a, b) => b.height - a.height,
  (a, b) => b.width * b.height - a.width * a.height,
  (a, b) => b.width - a.width,
]
export function arrangeWidgets(items = [], area, blocked = [], options = {}) {
  const tries = [(size) => size, (size) => (size === 'l' ? 'm' : size), () => 's']
  let best = null
  search: for (const shrink of tries) {
    const sizes = Object.fromEntries(items.map((it) => [it.id, shrink(it.size in SIZES ? it.size : 'm')]))
    const boxes = items.map((it) => ({ id: it.id, width: it.unit.width * SIZES[sizes[it.id]], height: it.unit.height * SIZES[sizes[it.id]] }))
    for (const order of ORDERS) {
      const got = packBoxes([...boxes].sort(order), area, blocked, options)
      if (!best || got.left.length < best.left.length) best = { ...got, sizes }
      if (!got.left.length) break search
    }
  }
  // the widgets left out keep their size: only a move that found them room changes it
  for (const id of best.left) best.sizes[id] = items.find((it) => it.id === id).size
  best.shrunk = items.filter((it) => !best.left.includes(it.id) && SIZES[best.sizes[it.id]] < SIZES[it.size in SIZES ? it.size : 'm']).map((it) => it.id)
  return best
}

// Does any box overlap another (`gap` 0: touching is fine)?
export function anyOverlap(boxes = []) {
  for (let i = 0; i < boxes.length; i++) {
    for (let j = i + 1; j < boxes.length; j++) {
      const a = boxes[i]
      const b = boxes[j]
      if (a.left < b.left + b.width && b.left < a.left + a.width && a.top < b.top + b.height && b.top < a.top + a.height) return true
    }
  }
  return false
}

// The settings change for an arrangement: `places` maps a widget id to its corner in pixels, `home`
// is HOME's width and height, `sizes` the size each arranged widget now has. Every other widget's
// place and size stay as they were.
export function arrangePatch(settings = {}, themeId, places = {}, home, sizes = {}) {
  let patch = { widgetPlaces: settings.widgetPlaces ?? {} }
  for (const [id, px] of Object.entries(places)) {
    const place = { left: round((px.left / home.width) * 100), top: round((px.top / home.height) * 100) }
    patch = { ...patch, ...placePatch({ ...settings, ...patch }, themeId, id, place) }
  }
  let withSizes = { ...settings, ...patch }
  for (const [id, size] of Object.entries(sizes)) withSizes = { ...withSizes, ...sizePatch(id, size, withSizes) }
  return { widgetPlaces: patch.widgetPlaces, widgetSizes: withSizes.widgetSizes ?? {} }
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
