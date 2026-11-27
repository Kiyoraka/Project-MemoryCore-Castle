<script setup>
// On the top bar when a newer Castle is out. Official builds: "Update ready" - the notes can be
// read first, and Update now saves the diary first: the companion is asked to save it, and Castle
// waits until the diary changes (three minutes at most, or Skip). Then the update downloads, its
// signature is checked, it installs and Castle restarts. A member's own build: a quiet "X is out"
// that opens Settings > About (it updates with git pull, never by itself).
// With "Update by itself" on (Settings > About), an official build presses Update now for the
// member once the companion has been quiet for QUIET_MS (autoupdate.js).
import { onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { diaryStamp, installCopies, terminalState, terminalWrite, updateCheck, updateInstall } from '../api/castle.js'
import { copiesChip, copiesLine } from './copies.js'
import { QUIET_MS, mayUpdateByItself } from './autoupdate.js'
import MarkdownText from './MarkdownText.vue'
import HelpTip from './HelpTip.vue'
import { typeLine } from './typeline.js'

const props = defineProps({
  // Settings > About > Update by itself
  auto: { type: Boolean, default: false },
  // the companion is working (TerminalPanel's busy)
  busy: { type: Boolean, default: false },
})
const emit = defineEmits(['status', 'about'])
const status = ref(null)
const step = ref('')
const notesOpen = ref(false)
// another copy of Castle on this PC: said before an update, and as its own chip
const copies = ref(null)
let skip = false
let timer
let quietTimer

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

async function check() {
  try {
    status.value = await updateCheck()
    emit('status', status.value)
  } catch {
    // no update line
  }
}

async function saveDiaryFirst() {
  const state = await terminalState().catch(() => null)
  if (!state?.running) return
  const before = await diaryStamp()
  step.value = 'Saving the diary first…'
  await typeLine(terminalWrite, 'save diary')
  for (let waited = 0; waited < 180 && !skip; waited += 3) {
    await sleep(3000)
    if ((await diaryStamp()) > before) {
      // give the companion a moment to finish writing
      await sleep(5000)
      return
    }
  }
}

async function updateNow() {
  skip = false
  try {
    await saveDiaryFirst()
    step.value = 'Downloading the update…'
    await updateInstall()
  } catch (reason) {
    step.value = String(reason)
  }
}

// Update by itself: any change (busy again, switched off, an update starting) restarts the quiet
// wait, so the update only begins after QUIET_MS without the companion working.
watch(
  () => [status.value, props.auto, props.busy, step.value],
  () => {
    clearTimeout(quietTimer)
    if (!mayUpdateByItself({ status: status.value, on: props.auto, busy: props.busy, step: step.value })) return
    quietTimer = setTimeout(() => {
      if (mayUpdateByItself({ status: status.value, on: props.auto, busy: props.busy, step: step.value })) updateNow()
    }, QUIET_MS)
  },
)

onMounted(() => {
  installCopies().then((c) => (copies.value = c)).catch(() => {})
  check()
  timer = setInterval(check, 6 * 60 * 60 * 1000)
})
onBeforeUnmount(() => {
  clearInterval(timer)
  clearTimeout(quietTimer)
})
</script>

<template>
  <div v-if="status?.available && status.canInstall" class="update">
    <template v-if="!step">
      <span>Update ready: {{ status.available }}</span>
      <button v-if="status.notes" class="quiet" @click="notesOpen = !notesOpen">What's new</button>
      <span v-if="copiesLine(copies)" class="warn" :title="copiesLine(copies)">{{ copiesChip(copies) }}</span>
      <button @click="updateNow">Update now</button>
      <HelpTip block text="The diary is saved first, then Castle restarts on the new version. Your memory, settings and keys stay as they are." />
    </template>
    <template v-else>
      <span>{{ step }}</span>
      <button v-if="step.startsWith('Saving')" @click="skip = true">Skip</button>
    </template>
    <div v-if="notesOpen && !step" class="notes">
      <MarkdownText :text="status.notes" />
    </div>
  </div>
  <button v-else-if="status?.available" class="update own" :title="status.message" @click="emit('about')">Castle {{ status.available }} is out</button>
  <button v-else-if="copiesLine(copies)" class="update own warn" :title="copiesLine(copies)" @click="emit('about')">{{ copiesChip(copies) }} of Castle</button>
</template>

<style scoped>
.update {
  position: fixed;
  top: 8px;
  right: 110px;
  z-index: 40;
  display: flex;
  gap: 8px;
  align-items: center;
  padding: 4px 10px;
  border: 1px solid var(--good);
  border-radius: 999px;
  background: var(--panel-background);
  font-size: 12px;
}
.update.own {
  border-color: var(--line);
  color: var(--text-dim);
  cursor: pointer;
}
.update.own:hover {
  color: var(--text);
}
.warn {
  color: var(--warn, #e0b060);
}
.update.own.warn {
  border-color: var(--warn, #e0b060);
}
.notes {
  position: absolute;
  top: calc(100% + 8px);
  right: 0;
  width: min(420px, 80vw);
  max-height: 60vh;
  overflow: auto;
  padding: 12px 16px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: var(--panel-background);
  -webkit-backdrop-filter: blur(var(--glass-blur)); backdrop-filter: blur(var(--glass-blur));
}
button.quiet {
  background: transparent;
  color: var(--text);
}
button {
  padding: 2px 10px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: var(--good);
  color: #07142e;
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}
</style>
