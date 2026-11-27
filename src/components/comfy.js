// Settings > ComfyUI, the pure parts: what the member chose (with the defaults an older settings file
// lacks), the choices its dropdowns offer, and the words its status says. The Rust core keeps and
// checks everything (src-tauri/src/comfy.rs); this only shapes the page, so it is tested without a window.

export const DEFAULT_ADDRESS = 'http://127.0.0.1:8188'

/** Everything the member can set; ComfyUI is off until they switch it on. */
export const EMPTY = { on: false, address: '', start: '', model: '', workflow: '', limitMinutes: 0, bindings: { prompt: [], width: [], height: [], seed: [], steps: [] } }

/** The saved choices, with every missing one at its default (a settings file from before ComfyUI has none). */
export const comfyOf = (settings) => ({ ...EMPTY, ...(settings?.comfy ?? {}) })

/** How long Castle waits for one picture; 0 is the usual fifteen minutes. */
export const LIMITS = [
  { value: 0, label: '15 minutes (usual)' },
  { value: 5, label: '5 minutes' },
  { value: 10, label: '10 minutes' },
  { value: 30, label: '30 minutes' },
  { value: 60, label: '1 hour' },
  { value: 120, label: '2 hours' },
  { value: 240, label: '4 hours' },
]

/** The limit dropdown, with a value the member set some other way kept rather than lost. */
export function limitOptions(minutes = 0) {
  if (LIMITS.some((option) => option.value === minutes)) return LIMITS
  const [usual, ...rest] = LIMITS
  return [usual, ...[...rest, { value: minutes, label: `${minutes} minutes` }].sort((a, b) => a.value - b.value)]
}

/**
 * The checkpoint dropdown: "the first one ComfyUI lists" (empty), then what ComfyUI lists. A name the
 * member saved that ComfyUI does not list now stays on the list, marked, so a ComfyUI that is switched
 * off at the moment never silently loses their choice.
 */
export function checkpointOptions(status, saved = '') {
  const listed = Array.isArray(status?.checkpoints) ? status.checkpoints : []
  const options = [{ value: '', label: 'The first one ComfyUI lists' }, ...listed.map((name) => ({ value: name, label: name }))]
  if (saved && !listed.includes(saved)) options.push({ value: saved, label: `${saved} (not listed now)` })
  return options
}

/**
 * What follows the switch: look whether ComfyUI answers and, when it was switched on, nothing answers
 * and a start file is set, start it. `check` gives the status, or nothing when it could not look.
 */
export async function afterSwitch(on, startFile, { check, start }) {
  const status = await check()
  if (on && status && !status.up && String(startFile ?? '').trim()) await start()
}

/** The small pill on the Connection row: { text, tone }. */
export function connectionPill(status, checking = false, starting = false) {
  if (starting) return { text: 'Starting…', tone: '' }
  if (checking) return { text: 'Checking…', tone: '' }
  if (!status) return { text: 'Not checked', tone: '' }
  return status.up ? { text: 'Answering', tone: 'good' } : { text: 'Not answering', tone: 'bad' }
}

/** The one line under the Connection row. */
export function connectionLine(status) {
  return status?.message || 'Press Check to see whether ComfyUI answers.'
}

/** The line under the Checkpoint row: why it may be idle, or what to do to fill it. */
export function modelLine(comfy, status) {
  if (comfy.workflow) return 'Your own workflow names its own model.'
  if (status?.up && !(status.checkpoints ?? []).length) return 'ComfyUI lists no checkpoint. Put one in its checkpoints folder.'
  if (!status?.up) return 'Check the connection to list your checkpoints.'
  return 'The model the plain workflow draws with.'
}

/** The last part of a path, for a line that names a file without printing the whole path. */
export function fileName(path = '') {
  return String(path).split(/[\\/]/).filter(Boolean).pop() ?? ''
}

// ---- Import: choosing which inputs of the member's own workflow take Castle's values ----

/** The groups the Import list offers, in the order it shows them. */
export const FIELD_GROUPS = [
  { id: 'prompt', title: 'The words to draw', note: 'Tick every field the prompt should go into.' },
  { id: 'width', title: 'Width', note: "Tick it to use Studio's size." },
  { id: 'height', title: 'Height', note: "Tick it to use Studio's size." },
  { id: 'seed', title: 'Seed', note: 'Tick it for a new picture every time.' },
  { id: 'steps', title: 'Steps', note: "Tick it to use Studio's quality as the number of steps." },
]

/** One input's identity: the node's id (written as text) and the input's name. */
export const fieldKey = (field) => JSON.stringify([field.node, field.input])

/**
 * What is ticked when the Import list opens: what the member chose before, else what Castle would
 * tick on its own, leaving out anything the file already handles with a {{placeholder}}.
 */
export function initialPicks(scan) {
  const picks = {}
  const chosen = scan?.bindings ?? {}
  const hasChoice = FIELD_GROUPS.some((g) => (chosen[g.id] ?? []).length)
  for (const group of FIELD_GROUPS) {
    const fields = scan?.[group.id] ?? []
    if (hasChoice) picks[group.id] = (chosen[group.id] ?? []).map(fieldKey)
    else if ((scan?.placeholders?.[group.id] ?? 0) > 0) picks[group.id] = []
    else picks[group.id] = fields.filter((f) => f.suggested).map(fieldKey)
  }
  return picks
}

/** The picks with one input switched on or off; the old picks are left as they were. */
export function toggled(picks, group, field, on) {
  const key = fieldKey(field)
  const rest = (picks[group] ?? []).filter((k) => k !== key)
  return { ...picks, [group]: on ? [...rest, key] : rest }
}

/** The picks as the core takes them: { prompt: [{ node, input }], ... }. */
export function picksToBindings(scan, picks) {
  const bindings = {}
  for (const group of FIELD_GROUPS) {
    const chosen = new Set(picks[group.id] ?? [])
    bindings[group.id] = (scan?.[group.id] ?? []).filter((f) => chosen.has(fieldKey(f))).map((f) => ({ node: f.node, input: f.input }))
  }
  return bindings
}

/** One field in the list: what it is, and what it says now, on one line. */
export function fieldLabel(field) {
  return `${field.title} · ${field.input}`
}
export function fieldNote(field) {
  const role = field.role === 'negative' ? 'negative prompt · ' : ''
  return `${role}now: ${field.preview}`
}

/** The line under "Where the words go": what the member chose, or what to do. */
export function bindingsLine(bindings = {}) {
  const count = (group) => (bindings[group] ?? []).length
  if (!FIELD_GROUPS.some((g) => count(g.id))) return 'Press Import to choose the fields, or put {{prompt}} in the file.'
  const parts = []
  if (count('prompt')) parts.push(count('prompt') === 1 ? 'prompt in 1 field' : `prompt in ${count('prompt')} fields`)
  if (count('width') || count('height')) parts.push("Studio's size")
  if (count('seed')) parts.push('new seed each time')
  if (count('steps')) parts.push('Studio quality as steps')
  return `Castle fills: ${parts.join(', ')}.`
}

/** The line under the Start file row: which file is set (the box is too narrow for a whole path). */
export function startLine(comfy) {
  const name = fileName(comfy.start.trim().replace(/^"|"$/g, ''))
  return name || 'Not set. You start ComfyUI yourself before drawing.'
}
