<script setup>
import { MAX_STEM_VOLUME, MODELS, VOCAL_GENDERS } from './musicActions.js'
import { sectionColor } from './colors.js'
import Knob from './Knob.vue'
import GlassSelect from '../../components/GlassSelect.vue'

// The bar along the bottom: the song API's own dials (model, voice, persona; five knobs for length,
// style weight, weirdness, audio weight and variety) and five faders for the first stems. Every
// change is dial.set or stem.volume.
const props = defineProps({
  dials: { type: Object, required: true },
  stems: { type: Array, default: () => [] },
  // what her hand touched just now ('dial:weirdness', 'stem:drums')
  touched: { type: String, default: '' },
})
const emit = defineEmits(['dial', 'volume'])

const KNOBS = [
  { name: 'length', label: 'LENGTH', min: 10, max: 360, step: 5, shown: (v) => `${Math.floor(v / 60)}:${String(v % 60).padStart(2, '0')}` },
  { name: 'styleWeight', label: 'STYLE', min: 0, max: 1, step: 0.01, shown: (v) => `${Math.round(v * 100)}%` },
  { name: 'weirdness', label: 'WEIRDNESS', min: 0, max: 1, step: 0.01, shown: (v) => `${Math.round(v * 100)}%` },
  { name: 'audioWeight', label: 'AUDIO REF', min: 0, max: 1, step: 0.01, shown: (v) => `${Math.round(v * 100)}%` },
  { name: 'variety', label: 'VARIETY', min: 0, max: 4, step: 1, shown: (v) => String(v) },
]
const VOICES = { f: 'Female', m: 'Male', any: 'Any' }

const faders = () => Array.from({ length: 5 }, (_, n) => props.stems[n] ?? null)
</script>

<template>
  <div class="dialbar">
    <div class="card">
      <span class="glyph" aria-hidden="true">&#8767;</span>
      <div class="fields">
        <label :class="{ hers: touched === 'dial:model' }">
          <span>MODEL</span>
          <GlassSelect class="model" :model-value="dials.model" :options="MODELS" aria-label="Model" @update:model-value="emit('dial', 'model', $event)" />
        </label>
        <div class="voices" :class="{ hers: touched === 'dial:vocalGender' }" role="group" aria-label="Voice">
          <button v-for="g in VOCAL_GENDERS" :key="g" :class="{ on: dials.vocalGender === g }" @click="emit('dial', 'vocalGender', g)">{{ VOICES[g] }}</button>
        </div>
        <label :class="{ hers: touched === 'dial:persona' }">
          <span>PERSONA</span>
          <input :value="dials.persona" placeholder="none" @change="emit('dial', 'persona', $event.target.value)" />
        </label>
      </div>
    </div>

    <div class="knobs">
      <Knob
        v-for="k in KNOBS"
        :key="k.name"
        :label="k.label"
        :value="dials[k.name]"
        :min="k.min"
        :max="k.max"
        :step="k.step"
        :shown="k.shown(dials[k.name])"
        :hers="touched === `dial:${k.name}`"
        @change="emit('dial', k.name, $event)"
      />
    </div>

    <div class="faders" :title="stems.length ? 'The first five stems' : 'The stems come in DRESS'">
      <div v-for="(s, n) in faders()" :key="n" class="fader" :class="{ hers: s && touched === `stem:${s.name}` }" :style="{ '--c': sectionColor(n) }">
        <input
          type="range"
          min="0"
          :max="MAX_STEM_VOLUME"
          step="0.01"
          :value="s ? s.volume : 0"
          :disabled="!s"
          :aria-label="s ? `${s.name} volume` : 'No stem'"
          @input="s && emit('volume', s.name, Number($event.target.value))"
        />
        <span>{{ s ? s.name.slice(0, 6) : '-' }}</span>
      </div>
    </div>
  </div>
</template>

<style scoped>
.dialbar {
  display: flex;
  align-items: center;
  gap: 18px;
  min-width: 0;
  padding: 10px 16px;
  border: 1px solid var(--line);
  border-radius: 16px;
  background: var(--panel-background);
  backdrop-filter: blur(var(--glass-blur));
  overflow-x: auto;
}

.card {
  display: flex;
  align-items: center;
  gap: 12px;
  padding-right: 16px;
  border-right: 1px solid var(--line);
}

.glyph {
  font-size: 28px;
  color: var(--accent);
  text-shadow: 0 0 12px color-mix(in srgb, var(--accent) 80%, transparent);
}

.fields {
  display: flex;
  flex-direction: column;
  gap: 5px;
}

label {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 10.5px;
  letter-spacing: 0.06em;
  color: var(--text-dim);
}

label span {
  width: 58px;
}

.model,
input:not([type]) {
  width: 110px;
  padding: 3px 6px;
  border: 1px solid var(--line);
  border-radius: 7px;
  background: var(--field, rgba(255, 255, 255, 0.06));
  color: var(--text);
  font: inherit;
  font-size: 12px;
}

.voices {
  display: flex;
  gap: 4px;
  padding-left: 64px;
}

.voices button {
  padding: 2px 9px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: none;
  color: var(--text-dim);
  font: inherit;
  font-size: 11px;
  cursor: pointer;
}

.voices button.on {
  background: color-mix(in srgb, var(--accent) 55%, transparent);
  color: var(--text);
}

.hers {
  animation: hers 1.6s ease-out;
  border-radius: 8px;
}

@keyframes hers {
  0% { box-shadow: 0 0 0 2px rgba(255, 210, 122, 0.9), 0 0 18px rgba(255, 210, 122, 0.7); }
  100% { box-shadow: none; }
}

.knobs {
  display: flex;
  gap: 14px;
  padding-right: 16px;
  border-right: 1px solid var(--line);
}

.faders {
  display: flex;
  gap: 12px;
}

.fader {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  font-size: 10px;
  color: var(--text-dim);
}

.fader input {
  writing-mode: vertical-lr;
  direction: rtl;
  width: 18px;
  height: 64px;
  accent-color: var(--c);
}

.fader input:disabled {
  opacity: 0.35;
}
</style>
