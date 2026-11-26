<script setup>
// The find bar every editor shares (Ctrl+F; Ctrl+H opens its replace row). It only asks: the room sends
// `<editor>.find { query, next, sel }` and `<editor>.replace { query, with, all, sel }`, which each editor
// answers in its own lane. An editor without them hears FIND_SOON.
import { nextTick, ref, watch } from 'vue'
import { FIND_SOON } from './ribbon.js'
import { icon } from './icons.js'

const props = defineProps({
  open: { type: Boolean, default: false },
  replace: { type: Boolean, default: false },
  // { find, replace } from ribbon.js findSupport
  support: { type: Object, default: () => ({ find: false, replace: false }) },
  busy: { type: Boolean, default: false },
})
const emit = defineEmits(['find', 'replace', 'close', 'update:replace'])

const query = ref('')
const withText = ref('')
const box = ref(null)

watch(
  () => [props.open, props.replace],
  ([open]) => {
    if (open) nextTick(() => box.value?.select?.())
  },
)

function find(next = true) {
  if (!query.value || !props.support.find) return
  emit('find', { query: query.value, next })
}

function replace(all = false) {
  if (!query.value || !props.support.replace) return
  emit('replace', { query: query.value, with: withText.value, all })
}

function onKey(e) {
  if (e.key === 'Escape') {
    e.preventDefault()
    emit('close')
  } else if (e.key === 'Enter') {
    e.preventDefault()
    find(!e.shiftKey)
  }
}
</script>

<template>
  <div v-if="open" class="find-bar" role="search" @keydown.stop="onKey">
    <template v-if="support.find">
      <div class="row">
        <span class="lead" v-html="icon('find', 16)"></span>
        <input ref="box" v-model="query" class="box" type="text" placeholder="Find" aria-label="Find" spellcheck="false" />
        <button title="Previous (Shift+Enter)" aria-label="Previous" :disabled="busy || !query" @click="find(false)" v-html="icon('up', 16)"></button>
        <button title="Next (Enter)" aria-label="Next" :disabled="busy || !query" @click="find(true)" v-html="icon('down', 16)"></button>
        <button title="Replace (Ctrl+H)" aria-label="Replace" :class="{ on: replace }" @click="emit('update:replace', !replace)" v-html="icon('replace', 16)"></button>
        <button title="Close (Esc)" aria-label="Close" @click="emit('close')" v-html="icon('close', 16)"></button>
      </div>
      <div v-if="replace" class="row">
        <span class="lead"></span>
        <template v-if="support.replace">
          <input v-model="withText" class="box" type="text" placeholder="Replace with" aria-label="Replace with" spellcheck="false" />
          <button class="word" :disabled="busy || !query" @click="replace(false)">Replace</button>
          <button class="word" :disabled="busy || !query" @click="replace(true)">All</button>
        </template>
        <span v-else class="soon">Replace comes to this editor soon</span>
      </div>
    </template>
    <div v-else class="row">
      <span class="lead" v-html="icon('find', 16)"></span>
      <span class="soon">{{ FIND_SOON }}</span>
      <button title="Close (Esc)" aria-label="Close" @click="emit('close')" v-html="icon('close', 16)"></button>
    </div>
  </div>
</template>

<style scoped>
.find-bar {
  position: absolute;
  top: 10px;
  right: 16px;
  z-index: 5;
  display: grid;
  gap: 4px;
  padding: 6px;
  border: 1px solid var(--scr-line);
  border-radius: 12px;
  background: rgba(16, 13, 44, 0.94);
  color: var(--scr-text);
  box-shadow: 0 10px 30px rgba(0, 0, 0, 0.4);
}
.row {
  display: flex;
  align-items: center;
  gap: 4px;
}
.lead {
  display: inline-flex;
  width: 20px;
  justify-content: center;
  color: var(--scr-muted);
}
.box {
  width: 200px;
  padding: 5px 8px;
  border: 1px solid var(--scr-line);
  border-radius: 8px;
  background: rgba(255, 255, 255, 0.06);
  color: var(--scr-text);
  font: inherit;
  font-size: 13px;
  outline: none;
}
.box:focus {
  border-color: var(--scr-accent);
}
button {
  display: inline-grid;
  place-items: center;
  min-width: 28px;
  height: 28px;
  padding: 0;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--scr-text);
  cursor: pointer;
}
button.word {
  padding: 0 10px;
  font: inherit;
  font-size: 12px;
}
button:hover:not(:disabled),
button.on {
  background: var(--scr-hover);
}
button:disabled {
  opacity: 0.4;
  cursor: default;
}
.soon {
  padding: 0 8px;
  color: var(--scr-muted);
  font-size: 13px;
}
</style>
