// STUDIO's workflows, the pure parts: which blocks the form draws, what a run sends, and what its
// buttons say. The Rust core checks and runs the file (src-tauri/src/workflows.rs); this only shapes
// the page.

export const ICONS = {
  picture: '🖼',
  people: '👥',
  person: '👤',
  poster: '📜',
  star: '★',
  heart: '♥',
  spark: '✦',
  brush: '🖌',
}
export const iconOf = (workflow) => ICONS[workflow?.icon] ?? ICONS.spark

const INPUTS = ['picture', 'character', 'text', 'chips']

const blocksOf = (workflow) => (Array.isArray(workflow?.blocks) ? workflow.blocks : [])
export const createOf = (workflow) => blocksOf(workflow).find((b) => b.type === 'create') ?? null

/** The blocks the form draws, in the file's order: every input and every Write-prompt button. */
export function fieldsOf(workflow) {
  return blocksOf(workflow)
    .filter((b) => INPUTS.includes(b.type) || b.type === 'describe')
    .map((b) => ({ ...b, label: (b.label ?? '').trim() || b.id }))
}

/** A fresh set of answers: empty words and pictures, each choice on its first option. */
export function emptyValues(workflow) {
  const values = {}
  for (const b of blocksOf(workflow)) {
    if (!INPUTS.includes(b.type)) continue
    values[b.id] = { text: b.type === 'chips' ? (b.options?.[0] ?? '') : '', name: '', picture: '' }
  }
  return values
}

/** The `{{names}}` the picture step's prompt uses, as block ids. */
export function usedIds(workflow) {
  const template = createOf(workflow)?.prompt ?? ''
  return [...template.matchAll(/\{\{\s*([^}]+?)\s*\}\}/g)].map((m) => m[1].split('.')[0])
}

/** The first thing still to fill before the picture can be made, in the member's words; '' when ready. */
export function missing(workflow, values) {
  const create = createOf(workflow)
  if (!create) return 'This workflow has no picture step.'
  const byId = Object.fromEntries(blocksOf(workflow).map((b) => [b.id, b]))
  const label = (id) => (byId[id]?.label ?? '').trim() || id
  for (const id of usedIds(workflow)) {
    if (byId[id]?.type === 'text' && !values[id]?.text?.trim()) return `Fill in ${label(id)}.`
  }
  for (const id of create.references ?? []) {
    if (!values[id]?.picture) return `Add a picture to ${label(id)}.`
  }
  return ''
}

/** Whether the workflow's picture step follows pictures the member adds (ComfyUI draws plain text pictures only). */
export const needsPictures = (workflow) => (createOf(workflow)?.references ?? []).length > 0

/**
 * The picture button: { kind, label }. kind is 'checking' (the sign-in check is still answering),
 * 'key' (no way to draw: the button opens Keys), 'busy', 'missing' (something to fill first) or 'ready'.
 * `way` is the Pictures power: 'ready' (the key), 'builtin' (Codex), 'prompt' (none). `comfy` is the
 * member's own ComfyUI being on: it opens the button for a workflow with no pictures to follow.
 */
export function createAction(workflow, values, way, loaded = true, busy = false, comfy = false) {
  if (!loaded) return { kind: 'checking', label: 'Checking…' }
  if (way !== 'ready' && way !== 'builtin' && !(comfy && !needsPictures(workflow))) return { kind: 'key', label: 'Add the OpenAI key' }
  if (busy) return { kind: 'busy', label: 'Working…' }
  const why = missing(workflow, values)
  return why ? { kind: 'missing', label: 'Create picture', why } : { kind: 'ready', label: 'Create picture' }
}

/** Write prompt from picture: ready when a way is open and its picture is chosen. */
export function describeAction(block, values, way, busy = false) {
  if (way !== 'ready' && way !== 'builtin') return { kind: 'key', label: block.label }
  if (busy) return { kind: 'busy', label: 'Reading the picture…' }
  if (!values[block.from]?.picture) return { kind: 'missing', label: block.label, why: 'Upload the picture first.' }
  return { kind: 'ready', label: block.label }
}

/** What a run sends: each block's words trimmed, empty parts left out. */
export function runValues(values) {
  const out = {}
  for (const [id, v] of Object.entries(values ?? {})) {
    const part = {}
    if (v.text?.trim()) part.text = v.text.trim()
    if (v.name?.trim()) part.name = v.name.trim()
    if (v.picture) part.picture = v.picture
    out[id] = part
  }
  return out
}

// ---------- the builder ----------

export const SIZES = [
  ['1024x1024', 'Square'],
  ['1024x1536', 'Tall'],
  ['1536x1024', 'Wide'],
]
export const QUALITIES = ['low', 'medium', 'high']
/** The shelf of blocks a member can add, in the words the builder shows. */
export const SHELF = [
  ['picture', 'Picture'],
  ['character', 'Character'],
  ['text', 'Text box'],
  ['chips', 'Choices'],
  ['describe', 'Write prompt from picture'],
]

/** A new workflow to start from: one text box and the picture step that draws it. */
export function newWorkflow() {
  return {
    schema: 1,
    name: '',
    icon: 'spark',
    power: 'image',
    description: '',
    blocks: [
      { id: 'scene', type: 'text', label: 'What to draw' },
      { id: 'make', type: 'create', prompt: '{{scene}}', references: [], size: '1024x1024', quality: 'medium' },
    ],
  }
}

/** A copy safe to edit in the builder. */
export const draftOf = (workflow) => JSON.parse(JSON.stringify(workflow ?? newWorkflow()))

function freeId(workflow, base) {
  const taken = new Set(blocksOf(workflow).map((b) => b.id))
  let n = 1
  while (taken.has(`${base}${n}`)) n++
  return `${base}${n}`
}

/** Adds a block of a kind just before the picture step; returns its id. */
export function addBlock(workflow, type) {
  const id = freeId(workflow, type === 'describe' ? 'read' : type)
  const label = Object.fromEntries(SHELF)[type] ?? type
  const block = { id, type, label: type === 'describe' ? 'Write prompt from picture' : label }
  if (type === 'chips') block.options = ['One', 'Two']
  if (type === 'describe') {
    block.from = blocksOf(workflow).find((b) => b.type === 'picture')?.id ?? ''
    block.into = blocksOf(workflow).find((b) => b.type === 'text')?.id ?? ''
  }
  const at = workflow.blocks.findIndex((b) => b.type === 'create')
  workflow.blocks.splice(at < 0 ? workflow.blocks.length : at, 0, block)
  return id
}

/** Takes a block away, and every reference to it from the picture step and the Write-prompt buttons. */
export function removeBlock(workflow, id) {
  workflow.blocks = workflow.blocks.filter((b) => b.id !== id || b.type === 'create')
  for (const b of workflow.blocks) {
    if (b.type === 'create') b.references = (b.references ?? []).filter((r) => r !== id)
    if (b.type === 'describe') {
      if (b.from === id) b.from = ''
      if (b.into === id) b.into = ''
    }
  }
}

/** Moves a block up (-1) or down (+1); the picture step stays last. */
export function moveBlock(workflow, id, step) {
  const i = workflow.blocks.findIndex((b) => b.id === id)
  const j = i + step
  if (i < 0 || j < 0 || j >= workflow.blocks.length) return
  if (workflow.blocks[i].type === 'create' || workflow.blocks[j].type === 'create') return
  const [b] = workflow.blocks.splice(i, 1)
  workflow.blocks.splice(j, 0, b)
}

/** The `{{names}}` the picture step's prompt can use, each with the words a chip shows. */
export function insertable(workflow) {
  const out = []
  for (const b of blocksOf(workflow)) {
    const label = (b.label ?? '').trim() || b.id
    if (b.type === 'text' || b.type === 'chips') out.push({ token: `{{${b.id}}}`, label })
    if (b.type === 'character') out.push({ token: `{{${b.id}.name}}`, label: `${label}'s name` })
  }
  return out
}

// ---------- ask the companion ----------

/** The brief that asks the companion to write one workflow file into the member's folder. */
export function companionBrief(words, folder) {
  const sep = folder.includes('\\') ? '\\' : '/'
  return [
    `Please make me a Castle STUDIO workflow: ${String(words).trim()}`,
    `Write it as one new folder inside ${folder} (name it with lowercase letters, digits and dashes) holding one file, workflow.json, for example ${folder}${sep}two-cats${sep}workflow.json. Touch nothing else.`,
    'The file is JSON: "schema": 1, "name", "icon" (one of picture, people, person, poster, star, heart, spark, brush), "power": "image", "description" (one line), and "blocks" in the order the form shows them.',
    'Each block has "id" (letters and digits), "type" and "label". Types: picture (an upload), character (a name and a reference picture), text (a text box), chips (with "options": up to 12 short words), describe (with "from": a picture block id and "into": a text block id; it writes a prompt from that picture).',
    'End with exactly one block of type create: "prompt" (it may use {{a-text-or-chips-id}} and {{a-character-id.name}}), "references" (up to five character or picture ids to follow), "size" (1024x1024, 1024x1536 or 1536x1024) and "quality" (low, medium or high).',
    'Draw nothing. Tell me when it is written.',
  ]
}
