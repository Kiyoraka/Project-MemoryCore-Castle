<script setup>
// The Sweep sheet's Developer section: build output and package caches in the DEV projects and their
// worktrees, with a Scan of its own. Collapsed to one line until opened; inside, the groups (On by
// default, Worktrees, Project build output), each collapsed to its summary. A worktree picks Leave,
// Build output or Whole worktree; what is in use or not safe shows disabled with the reason. Nothing
// is deleted until the member confirms, and every check runs again then.
import { computed, onMounted, ref } from 'vue'
import { useSweepDev } from '../../composables/useSweepDev.js'
import { sizeLabel } from './sweep.js'
import GlassSelect from '../GlassSelect.vue'
import { CHOICE_NAMES, DEV_GROUPS, clockOf, confirmLine, devSummary, groupSummary, worktreeLine } from './sweepdev.js'

const props = defineProps({
  // opened from the DEV room: the section starts open
  openAtStart: { type: Boolean, default: false },
})

const dev = useSweepDev()
const { scan, scanning, picks, cleaning, progress, result, problem, items, total } = dev

const open = ref(props.openAtStart)
const section = ref(null)
const asking = ref(false)

const line = computed(() => devSummary({ scanned: scan.value?.scanned, scanning: scanning.value, total: total.value }))
const scannedLine = computed(() => {
  const s = scan.value
  if (!s) return ''
  const p = `${s.projects} ${s.projects === 1 ? 'project' : 'projects'}`
  const w = `${s.worktrees} ${s.worktrees === 1 ? 'worktree' : 'worktrees'}`
  return `scanned ${clockOf(s.scanned)} · ${p}, ${w}`
})
const groups = computed(() =>
  DEV_GROUPS.map((g) => ({ ...g, items: items.value.filter((i) => i.group === g.id) })).filter((g) => g.items.length),
)
const ask = computed(() => confirmLine(items.value, picks.value))
const anyPicked = computed(() => ask.value !== 'Nothing is ticked.')

function onToggle(event) {
  open.value = event.target.open
  if (open.value && !scan.value && !scanning.value) dev.rescan()
}

function sizeOf(item) {
  if (item.kind === 'pnpm' && !item.bytesKnown) return 'unused'
  return sizeLabel(picks.value[item.id] === 'whole' ? item.wholeBytes : item.bytes)
}

function optionsOf(item) {
  return ['leave', ...item.choices].map((value) => ({ value, label: CHOICE_NAMES[value] }))
}

function noteOf(item) {
  return item.kind === 'worktree' ? worktreeLine(item) : item.detail
}

function tick(item, event) {
  dev.setPick(item.id, event.target.checked ? 'clean' : 'leave')
}

async function confirm() {
  asking.value = false
  await dev.clean()
}

onMounted(() => {
  if (!open.value) return
  section.value?.scrollIntoView?.({ block: 'nearest' })
  if (!scan.value) dev.rescan()
})
</script>

<template>
  <details ref="section" class="dev" :open="open" @toggle="onToggle">
    <summary><span class="title">{{ line }}</span></summary>

    <div class="bar">
      <button class="small" type="button" :disabled="scanning || cleaning" @click="dev.rescan()">{{ scanning ? 'Scanning…' : scan ? 'Scan again' : 'Scan' }}</button>
      <span class="dim">{{ scanning ? 'Reading the DEV projects and their worktrees…' : scannedLine }}</span>
    </div>
    <p v-if="scan && !items.length" class="dim">Nothing to clean in the DEV projects.</p>

    <details v-for="g in groups" :key="g.id" class="grp">
      <summary><b>{{ g.title }}</b> <span class="dim">{{ groupSummary(g.items, picks) }}</span></summary>
      <div v-for="item in g.items" :key="item.id" class="row" :class="{ off: item.blocked }">
        <input
          v-if="item.kind !== 'worktree'"
          type="checkbox"
          :aria-label="item.name"
          :checked="picks[item.id] === 'clean'"
          :disabled="!!item.blocked || cleaning"
          @change="tick(item, $event)"
        />
        <span v-else class="spacer" />
        <span class="name">
          {{ item.name }}
          <small>{{ noteOf(item) }}<span v-if="item.blocked" class="why"> · {{ item.blocked }}</span></small>
          <GlassSelect
            v-if="item.kind === 'worktree' && !item.blocked && item.choices.length"
            class="pick"
            :aria-label="`What to clean in ${item.name}`"
            :model-value="picks[item.id] ?? 'leave'"
            :options="optionsOf(item)"
            :disabled="cleaning"
            @update:model-value="dev.setPick(item.id, $event)"
          />
          <small v-else-if="item.kind === 'worktree' && !item.blocked">Nothing to clean</small>
        </span>
        <span class="size">{{ sizeOf(item) }}</span>
      </div>
    </details>

    <template v-if="scan && items.length">
      <div v-if="asking" class="confirm" role="alertdialog" aria-label="Clean the ticked developer items">
        <p>{{ ask }} Worktrees go through git worktree remove, branches stay, and every check runs again first.</p>
        <div class="bar">
          <button class="go" type="button" @click="confirm">Yes, clean</button>
          <button class="small" type="button" @click="asking = false">Not yet</button>
        </div>
      </div>
      <div v-else class="bar">
        <button class="go" type="button" :disabled="!anyPicked || cleaning || scanning" @click="asking = true">
          {{ cleaning ? 'Cleaning…' : `Clean ticked${total ? ` (${sizeLabel(total)})` : ''}` }}
        </button>
        <span class="dim">Nothing is deleted until you confirm.</span>
      </div>
    </template>

    <p v-if="cleaning" class="status">Cleaning {{ Math.min(progress.index + 1, progress.count) }} of {{ progress.count }}… {{ sizeLabel(progress.freed) }} freed</p>
    <p v-else-if="result" class="status good">{{ result }}</p>
    <p v-if="problem" class="status warn">{{ problem }}</p>
  </details>
</template>

<style scoped>
.dev {
  border-top: 1px solid var(--line);
  padding-top: 10px;
}
.dev > summary {
  cursor: pointer;
  font-size: 14px;
}
.title {
  font-weight: 500;
}
.bar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  margin-top: 8px;
}
.dim,
.name small {
  font-size: 12px;
  color: var(--text-dim);
}
.grp {
  margin-top: 8px;
  padding: 4px 10px;
  border: 1px solid var(--line);
  border-radius: 10px;
}
.grp > summary {
  cursor: pointer;
  padding: 4px 0;
  font-size: 13px;
}
.row {
  display: grid;
  grid-template-columns: 18px minmax(0, 1fr) auto;
  gap: 10px;
  align-items: start;
  padding: 7px 0;
  border-bottom: 1px dashed var(--line);
  font-size: 13px;
}
.row:last-child {
  border-bottom: 0;
}
.row.off {
  opacity: 0.6;
}
.row input {
  width: 16px;
  height: 16px;
  margin: 2px 0 0;
}
.name {
  display: flex;
  flex-direction: column;
  gap: 3px;
  overflow-wrap: anywhere;
}
.why {
  color: var(--warn);
}
.name :deep(.pick) {
  align-self: flex-start;
  font-size: 12px;
}
.size {
  color: var(--text-dim);
  white-space: nowrap;
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
.go {
  padding: 7px 14px;
  border: 0;
  border-radius: 10px;
  background: var(--accent);
  color: var(--accent-text, #fff);
  font: inherit;
  font-size: 13px;
  font-weight: 600;
  cursor: pointer;
}
.confirm {
  margin-top: 10px;
  padding: 10px 12px;
  border: 1px solid color-mix(in srgb, var(--accent) 60%, var(--line));
  border-radius: 10px;
  font-size: 13px;
}
.confirm p {
  margin: 0;
}
.status {
  margin: 8px 0 0;
  font-size: 13px;
}
.good {
  color: var(--good);
}
.warn {
  color: var(--warn);
}
button:disabled,
input:disabled {
  opacity: 0.55;
  cursor: default;
}
</style>
