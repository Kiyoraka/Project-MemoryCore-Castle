<script setup>
// HOME's Sweep card: one of three looks (Ring, Bar, Broom - the three kept designs), a gear to choose
// the look and whether the Recycle Bin starts ticked, and the preview sheet. The logic is shared in
// useSweep.js; the looks only draw. The look shown: the member's choice, else the theme's, else the bar.
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { useSweep } from '../../composables/useSweep.js'
import { GROUP_COLORS, LOOK_NAMES, LOOKS, barParts, resolveLook, sweptLine } from './sweep.js'
import SweepBar from './SweepBar.vue'
import SweepBroom from './SweepBroom.vue'
import SweepRing from './SweepRing.vue'
import HelpTip from '../HelpTip.vue'

const props = defineProps({
  themeLook: { type: String, default: '' },
})

const sweep = useSweep()
const { groups, measuring, status, cleaning, deepBusy, total, diskWarn } = sweep

const look = computed(() => resolveLook(status.value.settings.look, props.themeLook))
const parts = computed(() => barParts(groups.value))
const legend = computed(() => {
  const label = { temp: 'Temp', browser: 'Caches', thumbs: 'Thumbnails', crash: 'Logs', rust: 'Rust', recycle: 'Recycle Bin' }
  return parts.value.map((p) => ({ id: p.id, label: label[p.id] ?? p.id, color: GROUP_COLORS[p.id] }))
})
// "Last swept" moves on by itself while the card is shown
const now = ref(Date.now())
const swept = computed(() => sweptLine(status.value.lastSwept, now.value))

// the sheet itself is SweepSheetHost.vue, one for the whole window
function openSheet() {
  sweep.showSheet()
}

// ---------- the gear ----------
const card = ref(null)
const panel = ref(null)
const gearOpen = ref(false)
const panelAt = ref({ top: 0, left: 0 })

function toggleGear() {
  gearOpen.value = !gearOpen.value
  if (!gearOpen.value) return
  const box = card.value?.getBoundingClientRect()
  if (!box) return
  const width = 260
  const left = Math.min(Math.max(8, box.left), window.innerWidth - width - 8)
  const below = box.bottom + 6
  panelAt.value = { top: below + 220 > window.innerHeight ? Math.max(8, box.top - 226) : below, left }
}

function chooseLook(id) {
  sweep.saveSettings(id, status.value.settings.recycleBin)
}

function setRecycle(event) {
  sweep.saveSettings(status.value.settings.look, event.target.checked)
}

function closeOnOutside(event) {
  if (!gearOpen.value) return
  if (panel.value?.contains(event.target) || card.value?.contains(event.target)) return
  gearOpen.value = false
}

let detach = null
let clock = null
onMounted(() => {
  detach = sweep.attach()
  clock = setInterval(() => (now.value = Date.now()), 60000)
  document.addEventListener('pointerdown', closeOnOutside)
})
onBeforeUnmount(() => {
  detach?.()
  clearInterval(clock)
  document.removeEventListener('pointerdown', closeOnOutside)
})
</script>

<template>
  <div ref="card" class="widget sweep">
    <SweepRing v-if="look === 'ring'" :total="total" :parts="parts" :swept="swept" :measuring="measuring" :busy="cleaning || deepBusy" @sweep="openSheet" />
    <SweepBroom v-else-if="look === 'broom'" :total="total" :swept="swept" :measuring="measuring" :busy="cleaning || deepBusy" @sweep="openSheet" />
    <SweepBar v-else :total="total" :parts="parts" :legend="legend" :swept="swept" :measuring="measuring" :busy="cleaning || deepBusy" @sweep="openSheet" />

    <button class="gear" type="button" :class="{ on: gearOpen }" aria-label="Sweep settings" title="Sweep settings: its look and the Recycle Bin" :aria-expanded="gearOpen" @click="toggleGear">
      <svg viewBox="0 0 24 24" aria-hidden="true">
        <circle cx="12" cy="12" r="3" />
        <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
      </svg>
    </button>

    <Teleport to="body">
      <div v-if="gearOpen" ref="panel" class="sweep-panel" role="dialog" aria-label="Sweep settings" :style="{ top: `${panelAt.top}px`, left: `${panelAt.left}px` }">
        <span class="head">LOOK</span>
        <div class="looks">
          <button v-for="id in LOOKS" :key="id" type="button" class="look" :class="{ on: look === id }" :aria-pressed="look === id" @click="chooseLook(id)">{{ LOOK_NAMES[id] }}</button>
        </div>
        <p class="hint">{{ status.settings.look ? 'Your choice.' : themeLook ? "The theme's choice - pick one to keep it." : 'Pick the look you like.' }}</p>
        <span class="head with-tip">RECYCLE BIN<HelpTip block text="Emptying it can't be undone, so it starts unticked unless you choose this." /></span>
        <label class="toggle">
          <input type="checkbox" :checked="status.settings.recycleBin" @change="setRecycle" />
          <span>Start with the Recycle Bin ticked</span>
        </label>
      </div>
    </Teleport>

    <button v-if="diskWarn" class="disk-warn" type="button" title="Open Sweep at the Developer section: build output and caches in your DEV projects" @click="sweep.showSheet({ developer: true })">
      {{ diskWarn.line }} · Free disk space…
    </button>
  </div>
</template>

<style scoped>
.sweep {
  position: relative;
}
.disk-warn {
  display: block;
  width: calc(100% - 24px);
  margin: 0 12px 10px;
  padding: 5px 10px;
  border: 1px solid color-mix(in srgb, var(--warn, #f59e0b) 55%, var(--line));
  border-radius: 8px;
  background: color-mix(in srgb, var(--warn, #f59e0b) 14%, transparent);
  color: var(--warn, #f59e0b);
  font: inherit;
  font-size: 12px;
  text-align: left;
  cursor: pointer;
}
.gear {
  position: absolute;
  top: 8px;
  right: 8px;
  display: grid;
  place-items: center;
  width: 22px;
  height: 22px;
  padding: 0;
  border: 0;
  border-radius: 6px;
  background: none;
  color: var(--text-dim);
  cursor: pointer;
  opacity: 0;
  transition: opacity 0.15s;
}
.sweep:hover .gear,
.gear.on,
.gear:focus-visible {
  opacity: 1;
}
.gear svg {
  width: 15px;
  height: 15px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.8;
}
.sweep-panel {
  position: fixed;
  z-index: 60;
  display: flex;
  flex-direction: column;
  gap: 8px;
  width: 260px;
  padding: 14px;
  border: 1px solid var(--line);
  border-radius: 14px;
  background: var(--panel-background);
  -webkit-backdrop-filter: blur(var(--glass-blur)); backdrop-filter: blur(var(--glass-blur));
  box-shadow: 0 10px 30px rgba(0, 0, 0, 0.35);
  font-size: 13px;
}
.head {
  font-size: 11px;
  letter-spacing: 0.12em;
  color: var(--text-dim);
}
.head.with-tip {
  position: relative;
  display: flex;
  align-items: center;
}
.looks {
  display: flex;
  gap: 6px;
}
.look {
  flex: 1;
  padding: 6px 0;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: none;
  color: var(--text);
  font: inherit;
  cursor: pointer;
}
.look.on {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 20%, transparent);
}
.toggle {
  display: flex;
  gap: 8px;
  align-items: center;
  cursor: pointer;
}
.hint {
  margin: 0;
  font-size: 12px;
  color: var(--text-dim);
}
</style>
