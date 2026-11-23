import { describe, expect, it } from 'vitest'
import { ICONS, addBlock, companionBrief, createAction, describeAction, draftOf, emptyValues, fieldsOf, iconOf, insertable, missing, moveBlock, needsPictures, newWorkflow, removeBlock, runValues, usedIds } from './workflows.js'

const twoCharacters = {
  name: 'Two characters',
  icon: 'people',
  blocks: [
    { id: 'a', type: 'character', label: 'Character A' },
    { id: 'b', type: 'character', label: 'Character B' },
    { id: 'scene', type: 'text', label: 'Scene' },
    { id: 'style', type: 'chips', label: 'Style', options: ['Anime', 'Painterly', 'Photo'] },
    { id: 'make', type: 'create', prompt: '{{a.name}} and {{ b.name }}: {{scene}}. Style: {{style}}.', references: ['a', 'b'] },
  ],
}
const fromAPicture = {
  name: 'From a picture',
  blocks: [
    { id: 'photo', type: 'picture', label: 'Upload a picture' },
    { id: 'read', type: 'describe', label: 'Write prompt from picture', from: 'photo', into: 'prompt' },
    { id: 'prompt', type: 'text', label: '' },
    { id: 'make', type: 'create', prompt: '{{prompt}}', references: [] },
  ],
}

describe('workflows', () => {
  it('draws every input and Write-prompt button, never the picture step', () => {
    expect(fieldsOf(fromAPicture).map((f) => f.type)).toEqual(['picture', 'describe', 'text'])
    expect(fieldsOf(fromAPicture)[2].label).toBe('prompt') // an empty label falls back to the id
    expect(fieldsOf({})).toEqual([])
  })

  it('starts each choice on its first option and everything else empty', () => {
    const v = emptyValues(twoCharacters)
    expect(Object.keys(v)).toEqual(['a', 'b', 'scene', 'style'])
    expect(v.style.text).toBe('Anime')
    expect(v.a).toEqual({ text: '', name: '', picture: '' })
  })

  it('reads the names the prompt uses', () => {
    expect(usedIds(twoCharacters)).toEqual(['a', 'b', 'scene', 'style'])
  })

  it('says the first thing still to fill', () => {
    const v = emptyValues(twoCharacters)
    expect(missing(twoCharacters, v)).toBe('Fill in Scene.')
    v.scene.text = 'a picnic'
    expect(missing(twoCharacters, v)).toBe('Add a picture to Character A.')
    v.a.picture = 'C:/m/a.png'
    v.b.picture = 'C:/m/b.png'
    expect(missing(twoCharacters, v)).toBe('')
    expect(missing({ blocks: [] }, {})).toBe('This workflow has no picture step.')
  })

  it('the picture button follows the way to draw, then what is missing', () => {
    const v = emptyValues(twoCharacters)
    expect(createAction(twoCharacters, v, 'ready', false).kind).toBe('checking')
    expect(createAction(twoCharacters, v, 'prompt')).toEqual({ kind: 'key', label: 'Add the OpenAI key' })
    expect(createAction(twoCharacters, v, 'builtin', true, true).kind).toBe('busy')
    expect(createAction(twoCharacters, v, 'ready')).toMatchObject({ kind: 'missing', why: 'Fill in Scene.' })
    Object.assign(v.scene, { text: 'tea' })
    Object.assign(v.a, { picture: 'a.png' })
    Object.assign(v.b, { picture: 'b.png' })
    expect(createAction(twoCharacters, v, 'builtin')).toEqual({ kind: 'ready', label: 'Create picture' })
  })

  it("ComfyUI opens the picture button for a workflow with no pictures to follow, and only for that", () => {
    const plain = { blocks: [{ id: 'scene', type: 'text', label: 'Scene' }, { type: 'create', prompt: '{{scene}}', references: [] }] }
    const v = emptyValues(plain)
    v.scene.text = 'tea'
    expect(needsPictures(plain)).toBe(false)
    expect(needsPictures(twoCharacters)).toBe(true)
    expect(createAction(plain, v, 'prompt')).toEqual({ kind: 'key', label: 'Add the OpenAI key' })
    expect(createAction(plain, v, 'prompt', true, false, true)).toEqual({ kind: 'ready', label: 'Create picture' })
    // a workflow that follows pictures still needs Codex or the key: ComfyUI cannot draw those
    expect(createAction(twoCharacters, emptyValues(twoCharacters), 'prompt', true, false, true)).toEqual({ kind: 'key', label: 'Add the OpenAI key' })
    // reading a picture is not drawing: ComfyUI never opens Write prompt from picture
    expect(describeAction(fromAPicture.blocks[1], emptyValues(fromAPicture), 'prompt').kind).toBe('key')
  })

  it('Write prompt from picture waits for its picture', () => {
    const read = fromAPicture.blocks[1]
    const v = emptyValues(fromAPicture)
    expect(describeAction(read, v, 'prompt').kind).toBe('key')
    expect(describeAction(read, v, 'ready')).toMatchObject({ kind: 'missing', why: 'Upload the picture first.' })
    v.photo.picture = 'C:/m/photo.png'
    expect(describeAction(read, v, 'ready').kind).toBe('ready')
    expect(describeAction(read, v, 'ready', true).label).toBe('Reading the picture…')
  })

  it('a run sends trimmed words and leaves empty parts out', () => {
    expect(runValues({ a: { text: '', name: ' Mochi ', picture: 'a.png' }, scene: { text: '  tea  ', name: '', picture: '' } })).toEqual({
      a: { name: 'Mochi', picture: 'a.png' },
      scene: { text: 'tea' },
    })
  })

  it('every workflow has an icon, an unknown one the spark', () => {
    expect(iconOf(twoCharacters)).toBe(ICONS.people)
    expect(iconOf({ icon: 'dragon' })).toBe(ICONS.spark)
    expect(iconOf(null)).toBe(ICONS.spark)
  })

  it('the builder adds blocks before the picture step with fresh ids, and removing one unhooks it', () => {
    const w = newWorkflow()
    const a = addBlock(w, 'character')
    const b = addBlock(w, 'character')
    const p = addBlock(w, 'picture')
    const r = addBlock(w, 'describe')
    expect([a, b, r]).toEqual(['character1', 'character2', 'read1'])
    expect(w.blocks.at(-1).type).toBe('create')
    expect(w.blocks.find((x) => x.id === r)).toMatchObject({ from: p, into: 'scene' })
    w.blocks.at(-1).references = [a, b]
    removeBlock(w, a)
    removeBlock(w, p)
    removeBlock(w, 'make')
    expect(w.blocks.at(-1).references).toEqual([b])
    expect(w.blocks.find((x) => x.id === r).from).toBe('')
    expect(w.blocks.some((x) => x.type === 'create')).toBe(true) // the picture step cannot be removed
  })

  it('blocks move but the picture step stays last', () => {
    const w = newWorkflow()
    const c = addBlock(w, 'chips')
    moveBlock(w, c, -1)
    expect(w.blocks.map((x) => x.id)).toEqual([c, 'scene', 'make'])
    moveBlock(w, 'scene', 1)
    expect(w.blocks.at(-1).id).toBe('make')
  })

  it('offers the names a prompt can use and edits a copy, never the original', () => {
    expect(insertable(twoCharacters).map((n) => n.token)).toEqual(['{{a.name}}', '{{b.name}}', '{{scene}}', '{{style}}'])
    const copy = draftOf(twoCharacters)
    copy.blocks.pop()
    expect(twoCharacters.blocks).toHaveLength(5)
  })

  it('the companion is asked for one new folder in the member’s workflows and to draw nothing', () => {
    const lines = companionBrief('  two cats in a garden ', 'C:\\Castle\\workflows')
    expect(lines[0]).toBe('Please make me a Castle STUDIO workflow: two cats in a garden')
    expect(lines[1]).toContain('C:\\Castle\\workflows\\two-cats\\workflow.json')
    expect(companionBrief('x', '/home/rin/workflows')[1]).toContain('/home/rin/workflows/two-cats/workflow.json')
    expect(lines.join(' ')).toContain('exactly one block of type create')
    expect(lines.at(-1)).toContain('Draw nothing')
  })
})
