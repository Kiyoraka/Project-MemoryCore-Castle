<script setup>
// A small audio player in Castle's own colours (the browser's own is a white bar that ignores the
// theme): play / pause, a position slider and the time. Used for the voice line STUDIO just made.
import { onBeforeUnmount, ref, watch } from 'vue'
import { playerClock } from './playerclock.js'

const props = defineProps({ src: { type: String, required: true } })

const audio = ref(null)
const playing = ref(false)
const position = ref(0)
const length = ref(0)
let dragging = false

const toggle = () => {
  const el = audio.value
  if (!el) return
  if (el.paused) el.play().catch(() => {})
  else el.pause()
}
const onTime = () => {
  if (!dragging && audio.value) position.value = audio.value.currentTime
}
const onMeta = () => {
  length.value = audio.value && Number.isFinite(audio.value.duration) ? audio.value.duration : 0
}
const onEnded = () => {
  playing.value = false
  position.value = 0
}
const seek = (event) => {
  position.value = Number(event.target.value)
  if (audio.value) audio.value.currentTime = position.value
}

// a new clip starts from the top
watch(
  () => props.src,
  () => {
    position.value = 0
    length.value = 0
    playing.value = false
  },
)
onBeforeUnmount(() => audio.value?.pause())
</script>

<template>
  <div class="player">
    <audio
      ref="audio"
      :src="src"
      preload="metadata"
      @timeupdate="onTime"
      @loadedmetadata="onMeta"
      @durationchange="onMeta"
      @play="playing = true"
      @pause="playing = false"
      @ended="onEnded"
    ></audio>
    <button class="play" :aria-label="playing ? 'Pause' : 'Play'" @click="toggle">{{ playing ? '❚❚' : '▶' }}</button>
    <input
      class="slider"
      type="range"
      min="0"
      :max="length || 1"
      step="0.05"
      :value="position"
      :disabled="!length"
      aria-label="Position"
      @pointerdown="dragging = true"
      @pointerup="dragging = false"
      @input="seek"
    />
    <span class="time">{{ playerClock(position) }} / {{ playerClock(length) }}</span>
  </div>
</template>

<style scoped>
.player {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 6px 12px 6px 6px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: color-mix(in srgb, var(--accent) 8%, transparent);
  color: var(--text);
}
.play {
  display: grid;
  place-items: center;
  flex-shrink: 0;
  width: 30px;
  height: 30px;
  padding: 0;
  border: 1px solid var(--accent);
  border-radius: 50%;
  background: color-mix(in srgb, var(--accent) 18%, transparent);
  color: var(--accent);
  font-size: 11px;
  cursor: pointer;
}
.play:hover {
  background: color-mix(in srgb, var(--accent) 30%, transparent);
}
.slider {
  flex: 1;
  min-width: 0;
  height: 4px;
  accent-color: var(--accent);
  cursor: pointer;
}
.slider:disabled {
  cursor: default;
  opacity: 0.5;
}
.time {
  flex-shrink: 0;
  font-size: 12px;
  font-variant-numeric: tabular-nums;
  color: var(--text-dim);
}
</style>
