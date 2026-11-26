<script setup>
import { computed, ref, watch } from 'vue'
import { devFile, devReveal } from '../../api/devfiles.js'
import { photoOpenFromDev } from '../../api/castle.js'
import ModalSheet from '../../components/ModalSheet.vue'
import { absPath, fileFacts, textLines } from './tree.js'
import { isMac } from '../../platform.js'

// A project file in Castle's reader: text with line numbers in a monospace block (shown as plain
// text, never as markup), a picture as itself, or one line saying why not. Read only; Put in chat
// hands its whole path to the chat, Reveal in Explorer shows it in its folder, and →photo opens a
// png, jpg or webp in PHOTO with "Save back to <file>" (the first save keeps <name>.bak.<ext>).
const props = defineProps({
  // the DEV tab's slot
  slot: { type: Number, required: true },
  // the tab's project folder (for the whole path Put in chat sends)
  folder: { type: String, required: true },
  // the file from the tree: { name, rel }
  entry: { type: Object, required: true },
})
// put: the file's whole Windows path, for the chat; close: the reader is done
const emit = defineEmits(['put', 'close'])

const shown = ref(null)
const problem = ref('')
const revealing = ref(false)
const toPhoto = ref(false)
// the pictures PHOTO saves back in their own format
const editable = computed(() => /\.(png|jpe?g|webp)$/i.test(props.entry.name))

const lines = computed(() => (shown.value?.kind === 'text' ? textLines(shown.value.text) : []))
const facts = computed(() => fileFacts(shown.value, props.entry.name))

async function read() {
  shown.value = null
  problem.value = ''
  try {
    shown.value = await devFile(props.slot, props.entry.rel)
  } catch (reason) {
    problem.value = String(reason)
  }
}

async function reveal() {
  revealing.value = true
  try {
    await devReveal(props.slot, props.entry.rel)
  } catch (reason) {
    problem.value = String(reason)
  } finally {
    revealing.value = false
  }
}

async function openInPhoto() {
  toPhoto.value = true
  problem.value = ''
  try {
    await photoOpenFromDev(absPath(props.folder, props.entry.rel))
    emit('close')
  } catch (reason) {
    problem.value = String(reason)
  } finally {
    toPhoto.value = false
  }
}

watch(() => [props.slot, props.entry.rel], read, { immediate: true })
</script>

<template>
  <ModalSheet tag="FILE" :title="entry.rel" @close="emit('close')">
    <p v-if="problem" class="problem">{{ problem }}</p>
    <p v-if="!shown && !problem" class="note">Reading…</p>
    <img v-else-if="shown?.kind === 'picture'" class="picture" :src="shown.url" :alt="entry.name" />
    <p v-else-if="shown?.kind === 'note'" class="note">{{ shown.note }}</p>
    <pre v-else-if="shown?.kind === 'text'" class="code" :aria-label="entry.rel"><span v-for="(line, n) in lines" :key="n" class="line"><span class="number" aria-hidden="true">{{ n + 1 }}</span><span class="text">{{ line || ' ' }}</span></span></pre>

    <footer class="foot">
      <span class="facts">{{ facts }}</span>
      <span class="spacer"></span>
      <button v-if="editable" class="small" :disabled="toPhoto" title="Open it in PHOTO, with Save back to this file" @click="openInPhoto">&rarr;photo</button>
      <button class="small" @click="emit('put', absPath(folder, entry.rel))">Put in chat</button>
      <button class="small" :disabled="revealing" @click="reveal">{{ isMac ? 'Reveal in Finder' : 'Reveal in Explorer' }}</button>
    </footer>
  </ModalSheet>
</template>

<style scoped>
.code {
  margin: 0;
  font-family: 'Cascadia Mono', Consolas, ui-monospace, Menlo, monospace;
  font-size: 12.5px;
  line-height: 1.5;
  color: var(--text);
  white-space: normal;
}

.line {
  display: flex;
}

.number {
  flex: none;
  width: 48px;
  padding-right: 12px;
  text-align: right;
  color: var(--text-dim);
  user-select: none;
}

.text {
  flex: 1;
  min-width: 0;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.picture {
  display: block;
  max-width: 100%;
  margin: 0 auto;
  border-radius: 8px;
}

.note {
  margin: 8px 0;
  font-size: 13px;
  color: var(--text-dim);
}

.problem {
  margin: 8px 0;
  font-size: 13px;
  color: var(--warn);
}

.foot {
  position: sticky;
  bottom: 0;
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  margin-top: 12px;
  padding-top: 8px;
  border-top: 1px solid var(--line);
  background: var(--panel-background);
  font-size: 12.5px;
}

.facts {
  color: var(--text-dim);
}

.spacer {
  flex: 1;
}

.small {
  padding: 4px 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}

.small:hover:not(:disabled) {
  background: var(--hover);
}

.small:disabled {
  opacity: 0.5;
  cursor: wait;
}

.small:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
</style>
