<script setup>
import { onMounted, ref } from 'vue'
import { getSettings, keyStatus, saveSettings } from '../api/castle.js'
import { hasItsKey, isOn, resetPatch, roomIsOn, switchPatch, WIDGETS } from '../components/widgets/widgetplaces.js'
import { cellsOf, chosenSize, clampCell, gridPatch, MAX_H, resizeTo, sizeChoices, snapOf, snapPatch, spanOf, COLS } from '../components/widgets/widgetgrid.js'

// WIDGETS: HOME's widgets, each switched on or off and given a size; Arrange takes the member to
// HOME to drag them into place (one arrangement for every theme). Snap to grid on: sizes in whole
// cells (the w×h chips or a custom size); off: S, M or L, and any size by the corner on HOME.
const props = defineProps({
  themeId: { type: String, default: '' },
  themeName: { type: String, default: '' },
  // the theme's own widget places: a card or USAGE the theme has no place for is not drawn
  themeWidgets: { type: Object, default: () => ({}) },
})
const emit = defineEmits(['arrange', 'changed'])

const settings = ref({})
const savedKeys = ref([])
const problem = ref('')
const notice = ref('')

const hasPlaces = () => Object.keys(settings.value.widgetPlaces ?? {}).length > 0 || Object.keys(settings.value.widgetGrid ?? {}).length > 0
const snap = () => snapOf(settings.value)
const cellOf = (id) => settings.value.widgetGrid?.[id] ?? null
// the size shown for a widget not on HOME yet is its starting span
const sizeOf = (id) => cellOf(id) ?? clampCell({ id, ...spanOf(id), x: 0, y: 0 }, { snap: snap() })
const fmt = (n) => String(+Number(n).toFixed(2))
function sizeTo(id, w, h) {
  const grid = settings.value.widgetGrid ?? {}
  return save(gridPatch(grid, resizeTo(cellsOf(grid), id, w, h, { snap: snap() })))
}
// custom size, a whole cell at a time (snap on)
const canStep = (id, dw, dh) => {
  const c = sizeOf(id)
  const span = spanOf(id)
  const w = c.w + dw
  const h = c.h + dh
  return w >= span.minW && w <= COLS && h >= span.minH && h <= MAX_H
}
function snapTo(on) {
  const { patch, moved, pushed } = snapPatch(settings.value.widgetGrid ?? {}, on)
  const said = on
    ? moved || pushed
      ? `Snap to grid is on: ${moved} widget${moved === 1 ? '' : 's'} moved to the nearest cells, ${pushed} pushed down. None overlap.`
      : 'Snap to grid is on. Every widget already sat on whole cells.'
    : 'Snap to grid is off: on HOME, drag a widget or its corner anywhere. Widgets may overlap.'
  return save(patch, said)
}

async function load() {
  try {
    settings.value = (await getSettings()).settings
    savedKeys.value = (await keyStatus()).filter((k) => k.set).map((k) => k.name)
  } catch {
    // outside Castle's window: every widget on, medium
  }
}

async function save(patch, said = '') {
  problem.value = ''
  try {
    const { settings: now } = await getSettings()
    settings.value = (await saveSettings({ ...now, ...patch })).settings
    notice.value = said
    emit('changed')
  } catch (reason) {
    problem.value = String(reason?.message ?? reason)
  }
}

onMounted(load)
</script>

<template>
  <section class="widgets-room" aria-label="Widgets">
    <header class="top">
      <!-- the page's title and subtitle are Settings' own (SettingsPage) -->
      <p class="dim">Arrange moves them on HOME. They stay where you put them, whichever theme you choose.</p>
      <div class="actions">
        <button :disabled="!hasPlaces()" @click="save({ ...resetPatch(), widgetGrid: {} }, 'The layout is reset: each widget takes the theme\'s place once and stays there.')">Reset the layout</button>
        <button class="primary" @click="emit('arrange')">Arrange on HOME</button>
      </div>
    </header>
    <label class="snap">
      <span>
        <strong>Snap to grid</strong>
        <span class="dim small">{{ snap() ? 'Widgets move and resize by whole cells and never overlap.' : 'Widgets move and resize freely and may overlap.' }}</span>
      </span>
      <input type="checkbox" :checked="snap()" aria-label="Snap to grid" @change="snapTo($event.target.checked)" />
    </label>
    <p v-if="notice" class="notice">{{ notice }}</p>
    <p v-if="problem" class="problem" role="alert">{{ problem }}</p>

    <div class="grid">
      <article v-for="w in WIDGETS" :key="w.id" class="card" :class="{ off: !isOn(w.id, settings) }">
        <div class="head">
          <h3>{{ w.name }}</h3>
          <label class="switch">
            <input type="checkbox" :checked="isOn(w.id, settings)" :aria-label="`Show ${w.name} on HOME`" @change="save(switchPatch(w.id, $event.target.checked, settings))" />
            <span>{{ isOn(w.id, settings) ? 'On' : 'Off' }}</span>
          </label>
        </div>
        <p class="dim">{{ w.about }}</p>
        <p v-if="w.id === 'usage'" class="dim small">Off also stops Castle asking Claude Code and Codex for their limits.</p>
        <p v-if="!hasItsKey(w.id, savedKeys)" class="dim small">Shown once its key is saved in Settings &gt; Keys.</p>
        <p v-if="!roomIsOn(w.id, settings.rooms)" class="dim small">Shown while its room is on, in Settings &gt; Rooms.</p>
        <div class="sizes" role="group" :aria-label="`${w.name} size`">
          <button
            v-for="c in sizeChoices(w.id, snap())"
            :key="c.key"
            :class="{ on: chosenSize(w.id, sizeOf(w.id), snap()) === c.key }"
            :aria-pressed="chosenSize(w.id, sizeOf(w.id), snap()) === c.key"
            :disabled="!isOn(w.id, settings) || !c.ok"
            :title="c.ok ? '' : `${w.name} needs at least ${spanOf(w.id).minW}×${spanOf(w.id).minH}`"
            @click="sizeTo(w.id, c.w, c.h)"
          >
            {{ c.label }}
          </button>
          <span class="now dim small">{{ fmt(sizeOf(w.id).w) }}×{{ fmt(sizeOf(w.id).h) }}</span>
        </div>
        <div v-if="snap()" class="custom" role="group" :aria-label="`${w.name} custom size`">
          <span class="dim small">Wide</span>
          <button :disabled="!isOn(w.id, settings) || !canStep(w.id, -1, 0)" :aria-label="`${w.name} narrower`" @click="sizeTo(w.id, sizeOf(w.id).w - 1, sizeOf(w.id).h)">−</button>
          <button :disabled="!isOn(w.id, settings) || !canStep(w.id, 1, 0)" :aria-label="`${w.name} wider`" @click="sizeTo(w.id, sizeOf(w.id).w + 1, sizeOf(w.id).h)">+</button>
          <span class="dim small">Tall</span>
          <button :disabled="!isOn(w.id, settings) || !canStep(w.id, 0, -1)" :aria-label="`${w.name} shorter`" @click="sizeTo(w.id, sizeOf(w.id).w, sizeOf(w.id).h - 1)">−</button>
          <button :disabled="!isOn(w.id, settings) || !canStep(w.id, 0, 1)" :aria-label="`${w.name} taller`" @click="sizeTo(w.id, sizeOf(w.id).w, sizeOf(w.id).h + 1)">+</button>
        </div>
      </article>
    </div>
  </section>
</template>

<style scoped>
.widgets-room {
  display: flex;
  flex-direction: column;
  gap: 14px;
  min-width: 0;
  color: var(--text);
}

.top {
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  gap: 16px;
  flex-wrap: wrap;
}


h3 {
  margin: 0;
  font-size: 1rem;
  font-weight: 500;
}

.dim {
  color: var(--text-dim);
  margin: 4px 0;
  line-height: 1.4;
}

.small {
  font-size: 0.8rem;
}

.actions {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(min(100%, 230px), 1fr));
  gap: 12px;
}

.card {
  border: 1px solid var(--line);
  border-radius: 10px;
  background: rgba(0, 0, 0, 0.2);
  padding: 14px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.card.off {
  opacity: 0.6;
}

.head {
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.switch {
  display: flex;
  align-items: center;
  gap: 6px;
  font-size: 0.85rem;
  cursor: pointer;
}

.sizes {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: auto;
  padding-top: 6px;
}

.sizes button {
  min-width: 36px;
  padding: 4px 8px;
}

.sizes .now {
  align-self: center;
  margin-left: auto;
}

.custom {
  display: flex;
  align-items: center;
  gap: 6px;
}

.custom button {
  min-width: 28px;
  padding: 2px 6px;
}

.snap {
  display: flex;
  justify-content: space-between;
  align-items: center;
  gap: 16px;
  padding: 12px 14px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: rgba(0, 0, 0, 0.2);
  cursor: pointer;
}

.snap > span {
  display: flex;
  flex-direction: column;
  gap: 2px;
}

.sizes button.on {
  background: color-mix(in srgb, var(--accent) 30%, transparent);
  border-color: var(--accent);
}

.notice {
  color: var(--good, #9fe0b0);
  margin: 0;
}

.problem {
  color: var(--warn);
  margin: 0;
}

button {
  font: inherit;
  color: var(--text);
  background: var(--hover);
  border: 1px solid var(--line);
  border-radius: 8px;
  padding: 6px 14px;
  cursor: pointer;
}

button.primary {
  background: var(--accent);
  color: #07142e;
  border-color: transparent;
}

button:disabled {
  opacity: 0.45;
  cursor: default;
}
</style>
