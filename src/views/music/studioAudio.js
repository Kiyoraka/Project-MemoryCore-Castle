import { convertFileSrc } from '@tauri-apps/api/core'

// MUSIC STUDIO's sound: one AudioContext, a gain per voice. The chosen take plays alone, or the
// stems play together (each through its own gain, so mute, solo and volume move while it plays),
// and a laid loop rides under the intro or the outro. Files come through the asset protocol, as
// the Booth's songs do.

const CACHE = 10

export function createEngine({ onEnd } = {}) {
  let ctx = null
  const buffers = new Map() // path -> Promise<AudioBuffer>
  let voices = [] // { name, source, gain }
  let startedAt = 0
  let offset = 0
  let playing = false
  let generation = 0

  const context = () => (ctx ??= new AudioContext())

  /** The decoded file (cached by path). Throws when it cannot be read or decoded. */
  function load(path) {
    if (buffers.has(path)) return buffers.get(path)
    const loading = (async () => {
      const response = await fetch(convertFileSrc(path))
      if (!response.ok) throw new Error(`Could not read ${path.split(/[\\/]/).pop()}`)
      return context().decodeAudioData(await response.arrayBuffer())
    })()
    loading.catch(() => buffers.delete(path))
    buffers.set(path, loading)
    while (buffers.size > CACHE) buffers.delete(buffers.keys().next().value)
    return loading
  }

  function stopVoices() {
    for (const v of voices) {
      try {
        v.source.onended = null
        v.source.stop()
      } catch {
        // already stopped
      }
      v.source.disconnect()
      v.gain.disconnect()
    }
    voices = []
  }

  /**
   * Plays the tracks together from `at` seconds: tracks = [{ name, buffer, gain }], loops =
   * [{ name, buffer, from, to }] (a looping voice sounding only between from and to).
   */
  function play(tracks, at = 0, loops = []) {
    stopVoices()
    const c = context()
    c.resume().catch(() => {})
    generation += 1
    const mine = generation
    const now = c.currentTime + 0.03
    for (const t of tracks) {
      if (!t.buffer || at >= t.buffer.duration) continue
      const source = c.createBufferSource()
      source.buffer = t.buffer
      const gain = c.createGain()
      gain.gain.value = t.gain
      source.connect(gain).connect(c.destination)
      source.start(now, at)
      voices.push({ name: t.name, source, gain })
    }
    for (const l of loops) {
      if (!l.buffer || at >= l.to) continue
      const source = c.createBufferSource()
      source.buffer = l.buffer
      source.loop = true
      const gain = c.createGain()
      gain.gain.value = l.gain ?? 0.8
      source.connect(gain).connect(c.destination)
      source.start(now + Math.max(0, l.from - at), Math.max(0, at - l.from) % l.buffer.duration)
      source.stop(now + (l.to - at))
      voices.push({ name: l.name, source, gain, loop: true })
    }
    const main = voices.find((v) => !v.loop)
    if (!main) return false
    main.source.onended = () => {
      if (mine !== generation) return
      playing = false
      offset = 0
      stopVoices()
      onEnd?.()
    }
    startedAt = now
    offset = at
    playing = true
    return true
  }

  function position() {
    if (!playing || !ctx) return offset
    return offset + Math.max(0, ctx.currentTime - startedAt)
  }

  function pause() {
    offset = position()
    playing = false
    generation += 1
    stopVoices()
    return offset
  }

  function setOffset(at) {
    offset = at
  }

  /** Moves one voice's gain smoothly while it plays. */
  function setGain(name, value) {
    const voice = voices.find((v) => v.name === name && !v.loop)
    if (voice && ctx) voice.gain.gain.setTargetAtTime(value, ctx.currentTime, 0.03)
  }

  function close() {
    generation += 1
    stopVoices()
    ctx?.close().catch(() => {})
    ctx = null
    buffers.clear()
  }

  return { load, play, pause, position, setOffset, setGain, close, isPlaying: () => playing }
}
