import { describe, expect, it } from 'vitest'
import { EMPTY, FIELD_GROUPS, bindingsLine, checkpointOptions, comfyOf, connectionLine, connectionPill, fieldKey, fieldLabel, fieldNote, fileName, initialPicks, limitOptions, modelLine, picksToBindings, startLine, toggled } from './comfy.js'

const field = (node, input, extra = {}) => ({ node, input, classType: 'X', title: 'Node', preview: 'p', role: '', suggested: false, ...extra })
// what the core lists for a plain SDXL-style workflow
const scan = (extra = {}) => ({
  prompt: [field('2', 'text', { title: 'CLIPTextEncode', suggested: true, role: 'positive', preview: 'a lantern' }), field('3', 'text', { role: 'negative', preview: 'blurry' })],
  width: [field('4', 'width', { suggested: true })],
  height: [field('4', 'height', { suggested: true })],
  seed: [field('5', 'seed', { suggested: true })],
  steps: [field('5', 'steps')],
  placeholders: { prompt: 0, width: 0, height: 0, seed: 0, steps: 0 },
  bindings: { prompt: [], width: [], height: [], seed: [], steps: [] },
  ...extra,
})

describe('Settings > ComfyUI, the pure parts', () => {
  it('reads the saved choices, with ComfyUI off when an older settings file has none', () => {
    expect(comfyOf({})).toEqual(EMPTY)
    expect(comfyOf(undefined)).toEqual(EMPTY)
    expect(comfyOf({ comfy: { on: true, start: 'start.bat' } })).toEqual({ ...EMPTY, on: true, start: 'start.bat' })
    expect(EMPTY.on).toBe(false)
  })

  it('offers the usual fifteen minutes first and keeps a limit that was set some other way', () => {
    expect(limitOptions(0)[0]).toEqual({ value: 0, label: '15 minutes (usual)' })
    expect(limitOptions(30).map((o) => o.value)).toEqual([0, 5, 10, 30, 60, 120, 240])
    const odd = limitOptions(45)
    expect(odd.find((o) => o.value === 45)).toEqual({ value: 45, label: '45 minutes' })
    expect(odd.map((o) => o.value)).toEqual([0, 5, 10, 30, 45, 60, 120, 240])
  })

  it('lists the checkpoints ComfyUI has and never loses a saved name', () => {
    const up = { up: true, checkpoints: ['a.safetensors', 'b.ckpt'] }
    expect(checkpointOptions(up, '').map((o) => o.value)).toEqual(['', 'a.safetensors', 'b.ckpt'])
    expect(checkpointOptions(up, 'b.ckpt').filter((o) => o.value === 'b.ckpt')).toHaveLength(1)
    expect(checkpointOptions(null, 'old.safetensors')).toEqual([
      { value: '', label: 'The first one ComfyUI lists' },
      { value: 'old.safetensors', label: 'old.safetensors (not listed now)' },
    ])
    expect(checkpointOptions({ up: true }, '')).toHaveLength(1)
  })

  it('says whether ComfyUI answers, while checking, and before it was checked', () => {
    expect(connectionPill(null)).toEqual({ text: 'Not checked', tone: '' })
    expect(connectionPill(null, true)).toEqual({ text: 'Checking…', tone: '' })
    expect(connectionPill({ up: true })).toEqual({ text: 'Answering', tone: 'good' })
    expect(connectionPill({ up: false })).toEqual({ text: 'Not answering', tone: 'bad' })
    expect(connectionLine({ message: 'ComfyUI answers at http://127.0.0.1:8188 with 1 checkpoint.' })).toContain('with 1 checkpoint')
    expect(connectionLine(null)).toBe('Press Check to see whether ComfyUI answers.')
  })

  it('explains the checkpoint row by what is true now', () => {
    expect(modelLine({ workflow: 'W:\\q.json' }, { up: true, checkpoints: ['a'] })).toBe('Your own workflow names its own model.')
    expect(modelLine({ workflow: '' }, { up: true, checkpoints: [] })).toContain('lists no checkpoint')
    expect(modelLine({ workflow: '' }, null)).toBe('Check the connection to list your checkpoints.')
    expect(modelLine({ workflow: '' }, { up: false })).toBe('Check the connection to list your checkpoints.')
    expect(modelLine({ workflow: '' }, { up: true, checkpoints: ['a'] })).toBe('The model the plain workflow draws with.')
  })

  it('names a file by its last part, on either kind of path', () => {
    expect(fileName('W:\\ComfyUI_windows_portable\\run_amd_gpu_castle.bat')).toBe('run_amd_gpu_castle.bat')
    expect(fileName('/home/me/comfy/start.sh')).toBe('start.sh')
    expect(fileName('')).toBe('')
  })

  it('ticks on its own what Castle would suggest, never the negative prompt or the steps', () => {
    const picks = initialPicks(scan())
    expect(picks.prompt).toEqual([fieldKey(field('2', 'text'))])
    expect(picks.width).toEqual([fieldKey(field('4', 'width'))])
    expect(picks.seed).toEqual([fieldKey(field('5', 'seed'))])
    expect(picks.steps).toEqual([])
    expect(FIELD_GROUPS.map((g) => g.id)).toEqual(['prompt', 'width', 'height', 'seed', 'steps'])
  })

  it('leaves alone what the file already handles with a placeholder, and keeps an earlier choice as it was', () => {
    const handled = initialPicks(scan({ placeholders: { prompt: 1, width: 2, height: 0, seed: 0, steps: 0 } }))
    expect(handled.prompt).toEqual([])
    expect(handled.width).toEqual([])
    expect(handled.height).toEqual([fieldKey(field('4', 'height'))], 'the height has no placeholder, so it is still suggested')
    const before = scan({ bindings: { prompt: [{ node: '3', input: 'text' }], width: [], height: [], seed: [], steps: [{ node: '5', input: 'steps' }] } })
    const kept = initialPicks(before)
    expect(kept.prompt).toEqual([fieldKey(field('3', 'text'))])
    expect(kept.steps).toEqual([fieldKey(field('5', 'steps'))])
    expect(kept.seed).toEqual([], 'once the member chose, nothing is added behind their back')
  })

  it('switches one input on or off without touching the rest, and sends the choice as the core takes it', () => {
    const s = scan()
    let picks = initialPicks(s)
    picks = toggled(picks, 'prompt', s.prompt[1], true)
    expect(picks.prompt).toHaveLength(2)
    expect(toggled(picks, 'prompt', s.prompt[1], true).prompt).toHaveLength(2)
    picks = toggled(picks, 'prompt', s.prompt[0], false)
    expect(picks.prompt).toEqual([fieldKey(s.prompt[1])])
    expect(picks.width).toEqual(initialPicks(s).width)
    expect(picksToBindings(s, picks)).toEqual({
      prompt: [{ node: '3', input: 'text' }],
      width: [{ node: '4', input: 'width' }],
      height: [{ node: '4', input: 'height' }],
      seed: [{ node: '5', input: 'seed' }],
      steps: [],
    })
    // a tick for an input the scan no longer lists is dropped, not sent
    expect(picksToBindings(s, { prompt: [fieldKey(field('99', 'text'))] }).prompt).toEqual([])
  })

  it('tells nodes with the same input name apart by their id', () => {
    expect(fieldKey(field('6', 'text'))).not.toBe(fieldKey(field('7', 'text')))
    expect(fieldKey(field('12:3', 'text'))).not.toBe(fieldKey(field('12', '3:text')))
  })

  it('words a field and what the member chose', () => {
    expect(fieldLabel(field('2', 'text', { title: 'Positive prompt' }))).toBe('Positive prompt · text')
    expect(fieldNote(field('3', 'text', { role: 'negative', preview: 'blurry' }))).toBe('negative prompt · now: blurry')
    expect(fieldNote(field('2', 'text', { preview: 'a lantern' }))).toBe('now: a lantern')
    expect(bindingsLine({})).toContain('Press Import')
    expect(bindingsLine({ prompt: [{}], width: [{}], height: [{}], seed: [{}] })).toBe("Castle fills: prompt in 1 field, Studio's size, new seed each time.")
    expect(bindingsLine({ prompt: [{}, {}], steps: [{}] })).toBe('Castle fills: prompt in 2 fields, Studio quality as steps.')
    expect(bindingsLine({ prompt: [{}], width: [], height: [], seed: [], steps: [] }).length).toBeLessThanOrEqual(90)
  })

  it('names the start file that is set, or says there is none', () => {
    expect(startLine({ start: '' })).toContain('You start ComfyUI yourself')
    expect(startLine({ start: '   ' })).toContain('Not set')
    expect(startLine({ start: 'W:\\ComfyUI_windows_portable\\run_amd_gpu_castle.bat' })).toBe('run_amd_gpu_castle.bat')
    expect(startLine({ start: '"W:\\My Comfy\\start it.bat"' })).toBe('start it.bat')
    expect(startLine({ start: 'python main.py' })).toBe('python main.py')
  })
})
