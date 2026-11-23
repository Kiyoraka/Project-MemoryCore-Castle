import { describe, expect, it } from 'vitest'
import { EMPTY, checkpointOptions, comfyOf, connectionLine, connectionPill, fileName, limitOptions, modelLine, startLine } from './comfy.js'

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

  it('says what the start file does, and whether Castle started the running ComfyUI', () => {
    expect(startLine({ start: '' }, null)).toContain('you start ComfyUI yourself')
    expect(startLine({ start: 'a.bat' }, { startedByCastle: true })).toContain('Castle closes it')
    expect(startLine({ start: 'a.bat' }, { startedByCastle: false })).toContain('only when ComfyUI is not already running')
  })
})
