// STUDIO's Make bar (the chosen design, studio-room-1): one prompt box, four powers, and a main
// button whose words and action follow the power and what it can do now.

export const POWERS = [
  { id: 'image', label: 'Picture', key: 'OpenAI', ask: 'Draw me a picture of' },
  { id: 'video', label: 'Video', key: 'Seedance', ask: 'Make a short video of' },
  { id: 'song', label: 'Song', key: 'Suno', ask: 'Write and sing a song about' },
  { id: 'voice', label: 'Voice', key: 'ElevenLabs', ask: 'Say this out loud:' },
]

/** The small status pill of a power: { text, tone } (tone: 'ready', 'missing' or 'checking').
 * `loaded` is false only while the Picture power's real state (it asks Codex itself) is still
 * being read, so it reads "Checking..." instead of defaulting to "Add a key". */
export function powerPill(id, powers = {}, voiceUp = false, loaded = true) {
  if (id === 'image' && !loaded) return { text: 'Checking…', tone: 'checking' }
  if (id === 'voice' && voiceUp) return { text: 'VoiceStudio', tone: 'ready' }
  // the member's own ComfyUI draws a plain picture first, so it is what the Picture pill names
  if (id === 'image' && powers.comfy) return { text: 'ComfyUI', tone: 'ready' }
  const state = powers[id]
  if (state === 'builtin') return { text: 'Codex', tone: 'ready' }
  if (state === 'ready') return { text: 'Ready', tone: 'ready' }
  return { text: 'Add a key', tone: 'missing' }
}

/**
 * What the main button does for `id`:
 *  - kind 'image'     -> queue a picture (imageRequest)
 *  - kind 'voicestudio' -> speak with VoiceStudio on this computer
 *  - kind 'companion' -> ask the companion in the conversation (it runs its own helper and tells the cost)
 *  - kind 'key'       -> open Settings > Keys
 * with the button's label and, for the companion, the line to type.
 */
export function makeAction(id, powers = {}, voiceUp = false, companion = 'your companion', loaded = true) {
  if (id === 'image' && !loaded) return { kind: 'checking', label: 'Checking…' }
  const power = POWERS.find((p) => p.id === id) ?? POWERS[0]
  const state = powers[power.id]
  if (power.id === 'image') {
    return state === 'ready' || state === 'builtin' || powers.comfy ? { kind: 'image', label: 'Create picture' } : { kind: 'key', label: 'Add the OpenAI key' }
  }
  if (power.id === 'voice' && voiceUp) return { kind: 'voicestudio', label: 'Speak with VoiceStudio' }
  if (state === 'ready') return { kind: 'companion', label: `Ask ${companion}`, ask: power.ask }
  return { kind: 'key', label: `Add the ${power.key} key` }
}

/** The lines typed into the conversation for a companion-made thing. */
export function companionLines(ask, brief) {
  return [`${ask} ${String(brief ?? '').trim()}`, 'Tell me the cost first and ask before you make it.']
}
