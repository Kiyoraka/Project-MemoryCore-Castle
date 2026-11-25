<script setup>
// The translate row, to the pick browser-translate-2: "Japanese → English" (the target opens the
// language list), Translate or Original, Always for this site, and Read bubbles with its count.
// It acts on the page the toolbar points at; through the member's own Claude Code (browser_translate.rs).
import { computed } from 'vue'
import GlassSelect from '../components/GlassSelect.vue'
import { languageLabel, siteOf } from './browserview.js'

const props = defineProps({
  tab: { type: Object, required: true },
  settings: { type: Object, required: true },
  busy: { type: String, default: '' },
  said: { type: String, default: '' },
  bubbles: { type: Number, default: 0 },
})
const emit = defineEmits(['translate', 'original', 'language', 'always', 'bubbles', 'listing'])
const options = computed(() => (props.settings.languages ?? []).map(([value, label]) => ({ value, label })))
const from = computed(() => languageLabel(props.tab.lang, props.settings.languages))
const target = computed({
  get: () => props.settings.language,
  set: (code) => emit('language', code),
})
const always = computed(() => (props.settings.sites ?? []).includes(siteOf(props.tab.url)))
const bubbleHint = computed(() =>
  props.settings.bubbleWay === 'claude'
    ? 'Your Claude Code reads what this page shows and lays the words over each bubble (10 to 30 seconds)'
    : props.settings.bubbleWay === 'key'
      ? 'Your OpenAI key reads what this page shows and lays the words over each bubble (10 to 30 seconds)'
      : '',
)
</script>

<template>
  <div class="translate" role="region" aria-label="Translate this page">
    <div class="glass">
      <span class="from">{{ from }}</span>
      <svg width="16" height="10" viewBox="0 0 24 14" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><path d="M2 7h19M15 1l6 6-6 6" /></svg>
      <!-- while the list is open the pages step aside, so it can sit over them -->
      <GlassSelect v-model="target" class="to" :options="options" aria-label="Translate into" @open="emit('listing', $event)" />
      <template v-if="settings.canTranslate">
        <button v-if="!tab.translated" class="go" :disabled="!!busy" @click="emit('translate')">Translate</button>
        <button v-else class="go on" :disabled="!!busy" title="The page's own words back" @click="emit('original')">Original</button>
        <button v-if="settings.canKeepSites" class="always" role="switch" :aria-checked="always" title="Translate this site's pages by themselves" @click="emit('always', !always)">
          <span class="switch" :class="{ on: always }" aria-hidden="true"><i></i></span> Always
        </button>
      </template>
      <span v-else class="note">Translating pages needs Claude Code</span>
    </div>
    <div class="glass">
      <button class="read" :disabled="!!busy || !settings.bubbleWay" :title="bubbleHint || 'Reading bubbles needs Claude Code or an OpenAI key, on Windows'" @click="emit('bubbles')">
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" aria-hidden="true"><path d="M4 8V5a1 1 0 0 1 1-1h3M16 4h3a1 1 0 0 1 1 1v3M20 16v3a1 1 0 0 1-1 1h-3M8 20H5a1 1 0 0 1-1-1v-3" /><circle cx="12" cy="12" r="3.5" /></svg>
        Read bubbles
      </button>
      <span v-if="bubbles > 0" class="count">{{ bubbles }} bubble{{ bubbles === 1 ? '' : 's' }}</span>
    </div>
    <span v-if="busy || said" class="said" role="status">{{ busy || said }}</span>
  </div>
</template>

<style scoped>
.translate {
  height: 30px;
  flex-shrink: 0;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 0 12px;
  background: color-mix(in srgb, var(--background) 95%, transparent);
  border-bottom: 1px solid color-mix(in srgb, var(--accent) 18%, transparent);
  font-size: 12px;
  overflow: hidden;
}
.glass {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  height: 24px;
  padding: 0 4px 0 10px;
  border-radius: 12px;
  border: 1px solid color-mix(in srgb, var(--accent) 45%, transparent);
  background: color-mix(in srgb, var(--background) 85%, transparent);
  color: var(--text);
  flex-shrink: 0;
}
.from {
  color: var(--text);
}
.to {
  min-width: 150px;
}
button {
  height: 20px;
  padding: 0 10px;
  border-radius: 10px;
  border: 1px solid transparent;
  background: transparent;
  color: var(--text);
  font-size: 12px;
  cursor: pointer;
  display: inline-flex;
  align-items: center;
  gap: 6px;
}
button:disabled {
  opacity: 0.5;
  cursor: default;
}
.go {
  border-color: color-mix(in srgb, var(--accent) 60%, transparent);
  background: color-mix(in srgb, var(--accent) 21%, transparent);
}
.go.on {
  background: color-mix(in srgb, var(--accent) 36%, transparent);
}
.switch {
  width: 26px;
  height: 14px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--accent) 25%, transparent);
  display: inline-flex;
  align-items: center;
  padding: 2px;
  box-sizing: border-box;
}
.switch i {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: #fff;
  transition: transform 0.15s;
}
.switch.on {
  background: #3fcf9a;
}
.switch.on i {
  transform: translateX(12px);
}
.count {
  padding: 0 8px;
  margin-right: 2px;
  border-radius: 999px;
  background: #7ee0b8;
  color: #0b2a20;
  font-weight: 700;
  line-height: 18px;
}
.note,
.said {
  color: var(--text-dim);
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
</style>
