<script setup>
import { ref } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import { clipboardPaste } from '../../api/castle.js'
import { pathsIn } from './musicActions.js'

// SEED: what the song grows from - pictures, a hummed or played melody, MIDI, a short clip -
// dropped on the page, pasted here or typed as a path. Each add or remove is seed.add / seed.remove.
defineProps({
  seeds: { type: Array, default: () => [] },
  companion: { type: String, default: 'Your companion' },
  dropping: { type: Boolean, default: false },
  hers: { type: Boolean, default: false },
})
const emit = defineEmits(['add', 'remove'])

const typed = ref('')
const KIND_GLYPH = { picture: '\u25A3', audio: '\u266B', midi: '\u2669', video: '\u25B6' }
const fileName = (path) => path.split(/[\\/]/).pop()

async function onPaste(event) {
  const pastedText = event.clipboardData?.getData('text/plain')
  event.preventDefault()
  let text = pastedText
  if (!text) {
    try {
      const pasted = await clipboardPaste()
      text = pasted.kind === 'empty' ? '' : pasted.text
    } catch {
      text = ''
    }
  }
  for (const path of pathsIn(text)) emit('add', path)
}

function addTyped() {
  const paths = pathsIn(typed.value)
  for (const path of paths.length ? paths : [typed.value.trim()].filter(Boolean)) emit('add', path)
  typed.value = ''
}
</script>

<template>
  <div class="seed" :class="{ dropping, hers }" tabindex="0" @paste="onPaste">
    <div class="target">
      <strong>Drop or paste the seeds here</strong>
      <span>pictures (up to 5), a hummed or played melody, MIDI, one short clip. The feeling in words goes to {{ companion }} in the chat.</span>
    </div>
    <div class="typed">
      <input v-model="typed" placeholder="or a path: C:\Users\...\melody.mp3" @keydown.enter="addTyped" @paste.stop />
      <button :disabled="!typed.trim()" @click="addTyped">Add</button>
    </div>
    <ul v-if="seeds.length" class="list">
      <li v-for="s in seeds" :key="s.path">
        <img v-if="s.kind === 'picture'" :src="convertFileSrc(s.path)" alt="" loading="lazy" />
        <span v-else class="glyph" aria-hidden="true">{{ KIND_GLYPH[s.kind] }}</span>
        <span class="name" :title="s.path">{{ fileName(s.path) }}<small>{{ s.kind }}</small></span>
        <button class="remove" :aria-label="`Remove ${fileName(s.path)}`" @click="emit('remove', s.path)">&times;</button>
      </li>
    </ul>
    <p v-else class="quiet">No seeds yet - a song can grow from words alone too.</p>
  </div>
</template>

<style scoped>
.seed {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 12px;
  border: 1.5px dashed color-mix(in srgb, var(--accent) 45%, transparent);
  border-radius: 14px;
  outline: none;
  transition: border-color 0.2s, background 0.2s;
}

.seed:focus-visible,
.seed.dropping {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 10%, transparent);
}

.seed.hers {
  animation: hers 1.6s ease-out;
}

@keyframes hers {
  0% { box-shadow: 0 0 0 2px rgba(255, 210, 122, 0.9), 0 0 22px rgba(255, 210, 122, 0.6); }
  100% { box-shadow: none; }
}

.target {
  display: flex;
  flex-direction: column;
  gap: 2px;
  font-size: 12.5px;
  color: var(--text-dim);
}

.target strong {
  font-size: 14px;
  color: var(--text);
}

.typed {
  display: flex;
  gap: 6px;
}

.typed input {
  flex: 1;
  padding: 6px 9px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--field, rgba(255, 255, 255, 0.06));
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
}

.typed button,
.remove {
  padding: 5px 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}

.typed button:disabled {
  opacity: 0.4;
  cursor: default;
}

.list {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
  gap: 8px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.list li {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 6px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: rgba(0, 0, 0, 0.25);
}

.list img,
.glyph {
  flex: none;
  width: 40px;
  height: 40px;
  border-radius: 7px;
  object-fit: cover;
}

.glyph {
  display: grid;
  place-items: center;
  background: color-mix(in srgb, var(--accent) 20%, transparent);
  font-size: 18px;
}

.name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  font-size: 12px;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.name small {
  display: block;
  font-size: 10.5px;
  color: var(--text-dim);
}

.remove {
  padding: 2px 8px;
  font-size: 14px;
}

.quiet {
  margin: 0;
  font-size: 12.5px;
  font-style: italic;
  color: var(--text-dim);
}
</style>
