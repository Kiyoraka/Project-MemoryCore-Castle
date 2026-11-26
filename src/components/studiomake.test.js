import { describe, expect, it } from 'vitest'
import { POWERS, companionLines, makeAction, powerPill } from './studiomake.js'

describe("STUDIO's Make bar", () => {
  it('queues a picture with the key or on Codex, else asks for the key', () => {
    expect(makeAction('image', { image: 'ready' })).toEqual({ kind: 'image', label: 'Create picture' })
    expect(makeAction('image', { image: 'builtin' }).kind).toBe('image')
    expect(makeAction('image', { image: 'prompt' })).toEqual({ kind: 'key', label: 'Add the OpenAI key' })
  })
  it('asks the companion for a video or a song when its key is there', () => {
    expect(makeAction('video', { video: 'ready' }, false, 'Alice')).toEqual({ kind: 'companion', label: 'Ask Alice', ask: 'Make a short video of' })
    expect(makeAction('song', { song: 'prompt' })).toEqual({ kind: 'key', label: 'Add the Suno key' })
  })
  it('speaks with VoiceStudio first, then the companion with the key, then asks for it', () => {
    expect(makeAction('voice', { voice: 'prompt' }, true).kind).toBe('voicestudio')
    expect(makeAction('voice', { voice: 'ready' }, false).kind).toBe('companion')
    expect(makeAction('voice', {}, false)).toEqual({ kind: 'key', label: 'Add the ElevenLabs key' })
  })
  it('shows each power its pill', () => {
    expect(powerPill('image', { image: 'ready' })).toEqual({ text: 'Ready', tone: 'ready' })
    expect(powerPill('image', { image: 'builtin' }).text).toBe('Codex')
    expect(powerPill('video', {})).toEqual({ text: 'Add a key', tone: 'missing' })
    expect(powerPill('voice', {}, true).text).toBe('VoiceStudio')
  })
  it("shows Picture as checking while its real state is not read yet, and only Picture", () => {
    expect(makeAction('image', {}, false, 'Mira', false)).toEqual({ kind: 'checking', label: 'Checking…' })
    expect(powerPill('image', {}, false, false)).toEqual({ text: 'Checking…', tone: 'checking' })
    // the other three powers never spawn a process for their state, so they are never held back
    expect(makeAction('video', { video: 'ready' }, false, 'Mira', false).kind).toBe('companion')
    expect(powerPill('video', {}, false, false).tone).toBe('missing')
    // and once Picture's own state is in, it reads normally again
    expect(makeAction('image', { image: 'ready' }, false, 'Mira', true)).toEqual({ kind: 'image', label: 'Create picture' })
  })
  it("draws a plain picture with the member's ComfyUI even with no key, and names it on the pill", () => {
    expect(makeAction('image', { image: 'prompt', comfy: true })).toEqual({ kind: 'image', label: 'Create picture' })
    expect(powerPill('image', { image: 'prompt', comfy: true })).toEqual({ text: 'ComfyUI', tone: 'ready' })
    // ComfyUI first, even when Codex or the key could draw too
    expect(powerPill('image', { image: 'builtin', comfy: true }).text).toBe('ComfyUI')
    // it is the Picture power only: nothing else changes, and off it changes nothing
    expect(makeAction('video', { video: 'prompt', comfy: true })).toEqual({ kind: 'key', label: 'Add the Seedance key' })
    expect(powerPill('video', { comfy: true }).text).toBe('Add a key')
    expect(powerPill('image', { image: 'builtin', comfy: false }).text).toBe('Codex')
    // still held back while the real state is read
    expect(makeAction('image', { comfy: true }, false, 'Mira', false).kind).toBe('checking')
  })
  it('opens PAINT as a page of its own, always ready, whatever the keys', () => {
    expect(POWERS.map((p) => p.id)).toContain('paint')
    expect(makeAction('paint', {})).toEqual({ kind: 'paint', label: 'Paint' })
    expect(makeAction('paint', {}, false, 'Mira', false).kind).toBe('paint')
    expect(powerPill('paint', {})).toEqual({ text: 'Canvas', tone: 'ready' })
  })
  it('types the ask and the cost question', () => {
    expect(companionLines('Make a short video of', '  a lake at dawn ')).toEqual(['Make a short video of a lake at dawn', 'Tell me the cost first and ask before you make it.'])
  })
})
