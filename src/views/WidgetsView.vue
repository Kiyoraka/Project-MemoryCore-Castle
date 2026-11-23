<script setup>
import { onMounted, ref } from 'vue'
import { getSettings, keyStatus, saveSettings } from '../api/castle.js'
import { hasItsKey, isOn, resetPatch, roomIsOn, sizeOf, sizePatch, switchPatch, WIDGETS } from '../components/widgets/widgetplaces.js'

// WIDGETS: HOME's widgets, each switched on or off and given a size; Arrange takes the member to
// HOME to drag them into place (one arrangement for every theme).
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

const hasPlaces = () => Object.keys(settings.value.widgetPlaces ?? {}).length > 0

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
        <button :disabled="!hasPlaces()" @click="save(resetPatch(), 'The layout is reset: each widget takes the theme\'s place once and stays there.')">Reset the layout</button>
        <button class="primary" @click="emit('arrange')">Arrange on HOME</button>
      </div>
    </header>
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
            v-for="size in ['s', 'm', 'l']"
            :key="size"
            :class="{ on: sizeOf(w.id, settings) === size }"
            :aria-pressed="sizeOf(w.id, settings) === size"
            :disabled="!isOn(w.id, settings)"
            @click="save(sizePatch(w.id, size, settings))"
          >
            {{ size.toUpperCase() }}
          </button>
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
  gap: 6px;
  margin-top: auto;
  padding-top: 6px;
}

.sizes button {
  min-width: 36px;
  padding: 4px 8px;
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
