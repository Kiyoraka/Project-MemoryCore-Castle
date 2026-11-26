<script setup>
// Sweep's preview: each group with its size and a tick, laid out the way the chosen look draws it
// (Ring: a checklist; Bar: rows with size bars and switches; Broom: tiles). Nothing is deleted until
// Clean now. The Recycle Bin says it can't be undone; Deep clean (Windows) asks for administrator rights.
// Below them, the Developer section (SweepDeveloper.vue): the DEV projects' build output, its own Scan.
import { computed, onBeforeUnmount, onMounted } from 'vue'
import { GROUP_COLORS, GROUP_ICONS, sizeLabel } from './sweep.js'
import SweepDeveloper from './SweepDeveloper.vue'
import { useSweepDev } from '../../composables/useSweepDev.js'

const props = defineProps({
  look: { type: String, default: 'bar' },
  groups: { type: Array, default: () => [] },
  ticks: { type: Object, default: () => new Set() },
  tickedTotal: { type: Number, default: 0 },
  cleaning: { type: Boolean, default: false },
  freedSoFar: { type: Number, default: 0 },
  result: { type: String, default: '' },
  problem: { type: String, default: '' },
  deepOffered: { type: Boolean, default: false },
  deepBusy: { type: Boolean, default: false },
  // opened from the DEV room: the Developer section starts open
  developer: { type: Boolean, default: false },
})
const emit = defineEmits(['toggle', 'clean', 'deep', 'close'])

// The line under a group: the Recycle Bin can't be undone; else the group's own note (a browser
// open, a build running, still measuring); the Rust caches say what freeing them costs.
function noteOf(g) {
  if (g.id === 'recycle') return "Can't be undone"
  if (g.note) return g.note
  if (g.id === 'rust') return 'The next build of each project starts from scratch (a few minutes longer)'
  return ''
}

// Broom's tiles go three to a row (2 of 6 columns); a short last row stretches to fill it, so six
// groups (Windows) and fewer (macOS) both fill their rows.
function tileSpan(index, count) {
  const left = count % 3
  if (index < count - left) return 2
  return left === 1 ? 6 : 3
}

const largest = computed(() => Math.max(1, ...props.groups.map((g) => g.bytes || 0)))
const devCleaning = useSweepDev().cleaning
const busy = computed(() => props.cleaning || props.deepBusy || devCleaning.value)

function onKey(event) {
  if (event.key === 'Escape' && !busy.value) emit('close')
}
onMounted(() => window.addEventListener('keydown', onKey))
onBeforeUnmount(() => window.removeEventListener('keydown', onKey))
</script>

<template>
  <Teleport to="body">
    <div class="sweep-backdrop" role="dialog" aria-modal="true" aria-label="Sweep" @click.self="!busy && emit('close')">
      <article class="sweep-sheet" :class="look">
        <header>
          <h2><span class="spark">✦</span>Sweep</h2>
          <button class="close" type="button" aria-label="Close" :disabled="busy" @click="emit('close')">&times;</button>
        </header>

        <!-- Ring: a checklist -->
        <div v-if="look === 'ring'" class="list">
          <label v-for="g in groups" :key="g.id" class="row">
            <input type="checkbox" :checked="ticks.has(g.id)" :disabled="busy" @change="emit('toggle', g.id)" />
            <span class="name">{{ g.label }}<small v-if="noteOf(g)">{{ noteOf(g) }}</small></span>
            <span class="size">{{ sizeLabel(g.bytes) }}</span>
          </label>
        </div>

        <!-- Bar: rows with size bars and switches -->
        <div v-else-if="look === 'bar'" class="rows">
          <div v-for="g in groups" :key="g.id" class="row">
            <span class="icon" :style="{ color: GROUP_COLORS[g.id] }">{{ GROUP_ICONS[g.id] }}</span>
            <div class="middle">
              <div class="line"><span class="name">{{ g.label }}</span><span class="size">{{ sizeLabel(g.bytes) }}</span></div>
              <div class="meter"><span :style="{ width: `${(g.bytes / largest) * 100}%`, background: GROUP_COLORS[g.id] }" /></div>
              <small v-if="noteOf(g)">{{ noteOf(g) }}</small>
            </div>
            <input class="switch" type="checkbox" role="switch" :aria-label="g.label" :checked="ticks.has(g.id)" :disabled="busy" @change="emit('toggle', g.id)" />
          </div>
        </div>

        <!-- Broom: tiles -->
        <div v-else class="tiles">
          <button v-for="(g, i) in groups" :key="g.id" type="button" class="tile" :class="{ on: ticks.has(g.id) }" :style="{ gridColumn: `span ${tileSpan(i, groups.length)}` }" :disabled="busy" :aria-pressed="ticks.has(g.id)" @click="emit('toggle', g.id)">
            <span class="icon" :style="{ color: GROUP_COLORS[g.id] }">{{ GROUP_ICONS[g.id] }}</span>
            <span class="name">{{ g.label }}</span>
            <span class="size">{{ sizeLabel(g.bytes) }}</span>
            <small v-if="noteOf(g)">{{ noteOf(g) }}</small>
            <span class="tick" aria-hidden="true">{{ ticks.has(g.id) ? '✓' : '' }}</span>
          </button>
        </div>

        <SweepDeveloper :open-at-start="developer" />

        <p v-if="cleaning" class="status">Cleaning… {{ sizeLabel(freedSoFar) }} freed</p>
        <p v-else-if="result" class="status good">{{ result }}</p>
        <p v-if="problem" class="status warn">{{ problem }}</p>

        <footer>
          <button class="clean" type="button" :disabled="busy || !ticks.size" @click="emit('clean')">
            {{ cleaning ? 'Cleaning…' : `Clean now${tickedTotal ? ` (${sizeLabel(tickedTotal)})` : ''}` }}
          </button>
          <button v-if="deepOffered" class="deep" type="button" :disabled="busy" title="Windows' own Temp and Windows Update leftovers: Windows asks for administrator rights once" @click="emit('deep')">
            {{ deepBusy ? 'Deep cleaning…' : 'Deep clean' }}
          </button>
        </footer>
      </article>
    </div>
  </Teleport>
</template>

<style scoped>
.sweep-backdrop {
  position: fixed;
  inset: 0;
  z-index: 70;
  display: grid;
  place-items: center;
  background: color-mix(in srgb, var(--bg, #000) 45%, transparent);
  -webkit-backdrop-filter: blur(6px); backdrop-filter: blur(6px);
}
.sweep-sheet {
  display: flex;
  flex-direction: column;
  gap: 12px;
  width: min(560px, 92vw);
  max-height: 86vh;
  overflow: auto;
  padding: 18px 20px;
  border: 1px solid var(--line);
  border-radius: 16px;
  background: var(--panel-background);
  -webkit-backdrop-filter: blur(var(--glass-blur)); backdrop-filter: blur(var(--glass-blur));
  box-shadow: 0 14px 40px rgba(0, 0, 0, 0.4);
  box-sizing: border-box;
}
header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}
h2 {
  margin: 0;
  font-size: 20px;
  font-weight: 500;
}
.spark {
  margin-right: 10px;
  color: var(--accent);
}
.close {
  border: 0;
  background: none;
  color: var(--text-dim);
  font-size: 22px;
  cursor: pointer;
}
.name small,
.middle small,
.tile small {
  display: block;
  font-size: 11px;
  color: var(--text-dim);
}
.size {
  color: var(--text-dim);
  white-space: nowrap;
}

/* Ring: a checklist */
.list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.list .row {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 10px 14px;
  border: 1px solid var(--line);
  border-radius: 10px;
  cursor: pointer;
}
.list .name {
  flex: 1;
}
.list input {
  width: 18px;
  height: 18px;
}

/* Bar: rows with size bars and switches */
.rows {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
.rows .row {
  display: flex;
  align-items: center;
  gap: 12px;
}
.rows .icon,
.tiles .icon {
  display: grid;
  place-items: center;
  width: 34px;
  height: 34px;
  border-radius: 9px;
  background: color-mix(in srgb, var(--line) 40%, transparent);
  font-size: 16px;
}
.middle {
  flex: 1;
  display: flex;
  flex-direction: column;
  gap: 5px;
}
.line {
  display: flex;
  justify-content: space-between;
}
.meter {
  height: 5px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--line) 50%, transparent);
  overflow: hidden;
}
.meter span {
  display: block;
  height: 100%;
}
.switch {
  appearance: none;
  position: relative;
  width: 38px;
  height: 22px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--line) 70%, transparent);
  cursor: pointer;
  transition: background 0.15s;
}
.switch::after {
  content: '';
  position: absolute;
  top: 3px;
  left: 3px;
  width: 16px;
  height: 16px;
  border-radius: 50%;
  background: var(--text);
  transition: transform 0.15s;
}
.switch:checked {
  background: var(--accent);
}
.switch:checked::after {
  transform: translateX(16px);
}

/* Broom: tiles */
.tiles {
  display: grid;
  grid-template-columns: repeat(6, 1fr);
  gap: 10px;
}
.tile {
  position: relative;
  grid-column: span 2;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 6px;
  padding: 12px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: none;
  color: var(--text);
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.tile.on {
  border-color: color-mix(in srgb, var(--accent) 60%, var(--line));
}
.tick {
  position: absolute;
  right: 10px;
  bottom: 10px;
  display: grid;
  place-items: center;
  width: 20px;
  height: 20px;
  border: 1px solid var(--line);
  border-radius: 50%;
  font-size: 12px;
}
.tile.on .tick {
  border-color: transparent;
  background: var(--good, #34d399);
  color: #0b1a14;
}

.status {
  margin: 0;
  font-size: 13px;
}
.good {
  color: var(--good);
}
.warn {
  color: var(--warn);
}
footer {
  display: flex;
  gap: 12px;
  align-items: center;
}
.clean {
  flex: 1;
  padding: 11px 0;
  border: 0;
  border-radius: 12px;
  background: linear-gradient(90deg, var(--accent), var(--sweep-browser, #38bdf8));
  color: var(--accent-text, #fff);
  font: inherit;
  font-weight: 600;
  cursor: pointer;
}
.deep {
  padding: 10px 16px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: none;
  color: var(--text);
  font: inherit;
  cursor: pointer;
}
.broom footer {
  flex-direction: column;
}
.broom .clean {
  width: 100%;
}
.broom .deep {
  border: 0;
  text-decoration: underline;
  color: var(--accent);
}
button:disabled {
  opacity: 0.55;
  cursor: default;
}
</style>
