<script setup>
import { MAX_STEM_VOLUME, isVocal, stemGain } from './musicActions.js'
import { sectionColor, stemGlyph } from './colors.js'

// The stem lanes under the timeline: icon, mute dot, name, its waveform, a solo button and a
// volume slider. Each control is an action (stem.mute, stem.solo, stem.volume); the page plays them
// together through one AudioContext with a gain per stem.
const props = defineProps({
  stems: { type: Array, default: () => [] },
  // name -> peaks (0..1)
  peaks: { type: Object, default: () => ({}) },
  karaoke: { type: Boolean, default: false },
  // what her hand touched just now ('stem:drums')
  touched: { type: String, default: '' },
})
const emit = defineEmits(['mute', 'solo', 'volume'])

const bars = (name) => props.peaks[name] ?? []
const silent = (s) => stemGain(s, props.stems, props.karaoke) === 0
</script>

<template>
  <div class="lanes">
    <div
      v-for="(s, n) in stems"
      :key="s.name"
      class="lane"
      :class="{ silent: silent(s), hers: touched === `stem:${s.name}` }"
      :style="{ '--c': sectionColor(n) }"
    >
      <span class="glyph" aria-hidden="true">{{ stemGlyph(s.name) }}</span>
      <button
        class="mute"
        :class="{ on: !s.muted }"
        :aria-label="s.muted ? `Unmute ${s.name}` : `Mute ${s.name}`"
        :title="s.muted ? 'Muted - click to unmute' : 'Click to mute'"
        @click="emit('mute', s.name, !s.muted)"
      ></button>
      <span class="name">
        {{ s.name }}
        <small v-if="karaoke && isVocal(s.name)">karaoke</small>
      </span>
      <svg class="wave" viewBox="0 0 200 40" preserveAspectRatio="none" aria-hidden="true">
        <rect
          v-for="(p, i) in bars(s.name)"
          :key="i"
          :x="(i * 200) / bars(s.name).length"
          :y="20 - Math.max(0.6, p * 18)"
          :width="Math.max(0.4, 200 / bars(s.name).length - 0.3)"
          :height="Math.max(1.2, p * 36)"
        />
        <rect v-if="!bars(s.name).length" x="0" y="19.5" width="200" height="1" />
      </svg>
      <button
        class="solo"
        :class="{ on: s.solo }"
        :aria-label="s.solo ? `Take ${s.name} out of solo` : `Solo ${s.name}`"
        @click="emit('solo', s.name, !s.solo)"
      >S</button>
      <input
        class="volume"
        type="range"
        min="0"
        :max="MAX_STEM_VOLUME"
        step="0.01"
        :value="s.volume"
        :aria-label="`${s.name} volume`"
        :title="`${Math.round(s.volume * 100)}%`"
        @input="emit('volume', s.name, Number($event.target.value))"
      />
    </div>
    <slot v-if="!stems.length" />
  </div>
</template>

<style scoped>
.lanes {
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.lane {
  display: grid;
  grid-template-columns: 26px 16px minmax(70px, 120px) 1fr 26px minmax(80px, 140px);
  align-items: center;
  gap: 10px;
  padding: 6px 12px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: color-mix(in srgb, var(--c) 7%, rgba(0, 0, 0, 0.3));
  transition: opacity 0.25s, box-shadow 0.4s;
}

.lane.silent {
  opacity: 0.45;
}

.lane.hers {
  animation: hers 1.6s ease-out;
}

@keyframes hers {
  0% { box-shadow: 0 0 0 2px rgba(255, 210, 122, 0.9), 0 0 24px rgba(255, 210, 122, 0.7); }
  100% { box-shadow: none; }
}

.glyph {
  font-size: 18px;
  color: var(--c);
  text-align: center;
}

.mute {
  width: 12px;
  height: 12px;
  padding: 0;
  border: 1.5px solid var(--c);
  border-radius: 50%;
  background: transparent;
  cursor: pointer;
}

.mute.on {
  background: var(--c);
  box-shadow: 0 0 8px var(--c);
}

.name {
  overflow: hidden;
  font-size: 12.5px;
  text-overflow: ellipsis;
  white-space: nowrap;
  text-transform: capitalize;
}

.name small {
  display: block;
  font-size: 10px;
  color: var(--text-dim);
  text-transform: none;
}

.wave {
  width: 100%;
  height: 30px;
  fill: var(--c);
  filter: drop-shadow(0 0 3px color-mix(in srgb, var(--c) 60%, transparent));
}

.solo {
  width: 24px;
  height: 22px;
  padding: 0;
  border: 1px solid var(--line);
  border-radius: 6px;
  background: none;
  color: var(--text-dim);
  font: inherit;
  font-size: 11px;
  cursor: pointer;
}

.solo.on {
  border-color: #ffd27a;
  background: rgba(255, 210, 122, 0.25);
  color: #ffe6b0;
}

.volume {
  width: 100%;
  accent-color: var(--accent);
}
</style>
