// Settings > ComfyUI, the pure parts: what the member chose (with the defaults an older settings file
// lacks), the choices its dropdowns offer, and the words its status says. The Rust core keeps and
// checks everything (src-tauri/src/comfy.rs); this only shapes the page, so it is tested without a window.

export const DEFAULT_ADDRESS = 'http://127.0.0.1:8188'

/** Everything the member can set; ComfyUI is off until they switch it on. */
export const EMPTY = { on: false, address: '', start: '', model: '', workflow: '', limitMinutes: 0 }

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

/** The small pill on the Connection row: { text, tone }. */
export function connectionPill(status, checking = false) {
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

/** The line under the Start file row: which file is set (the box is too narrow for a whole path). */
export function startLine(comfy) {
  const name = fileName(comfy.start.trim().replace(/^"|"$/g, ''))
  return name || 'Not set. You start ComfyUI yourself before drawing.'
}
