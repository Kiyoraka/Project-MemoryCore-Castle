<script setup>
// The address bar's suggestions, to the pick browser-suggest-3: past searches as pills with an x on
// top, sites and bookmarks as slim cards below, the selected one in a mint outline. The pages step
// aside while it is open (browser_chrome), so it can sit over them.
import { computed } from 'vue'
import { suggestionGroups } from './browserview.js'

const props = defineProps({
  rows: { type: Array, required: true },
  // the selected row's place in the shown order, -1 for none
  selected: { type: Number, default: -1 },
})
const emit = defineEmits(['pick', 'forget', 'hover'])
const groups = computed(() => suggestionGroups(props.rows))
const at = (row) => groups.value.order.indexOf(row)
const letter = (row) => (row.text.replace(/^https?:\/\/(www\.)?/, '')[0] ?? '?').toUpperCase()
</script>

<template>
  <div class="suggest" role="listbox" aria-label="Suggestions" @mousedown.prevent>
    <div v-if="groups.pills.length" class="pills">
      <div v-for="row in groups.pills" :key="row.url" class="pill" :class="{ on: at(row) === selected }" role="option" :aria-selected="at(row) === selected" @mouseenter="emit('hover', at(row))" @click="emit('pick', row)">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></svg>
        <span class="words">{{ row.text }}</span>
        <button class="x" :aria-label="`Forget ${row.text}`" title="Forget this search" @click.stop="emit('forget', row)">
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.6" stroke-linecap="round" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
        </button>
      </div>
    </div>
    <div v-for="row in groups.cards" :key="row.url" class="card" :class="{ on: at(row) === selected }" role="option" :aria-selected="at(row) === selected" @mouseenter="emit('hover', at(row))" @click="emit('pick', row)">
      <span class="letter" aria-hidden="true">{{ letter(row) }}</span>
      <span class="text">
        <span class="title">{{ row.text }}</span>
        <span v-if="row.sub" class="sub">{{ row.sub }}</span>
      </span>
      <svg v-if="row.kind === 'bookmark'" class="star" width="16" height="16" viewBox="0 0 24 24" fill="currentColor" aria-label="Bookmark"><path d="M12 3l2.8 5.8 6.2.9-4.5 4.4 1 6.2L12 17.4 6.5 20.3l1-6.2L3 9.7l6.2-.9z" /></svg>
    </div>
  </div>
</template>

<style scoped>
.suggest {
  position: absolute;
  z-index: 20;
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px;
  border-radius: 14px;
  border: 1px solid var(--line);
  background: color-mix(in srgb, var(--background) 97%, transparent);
  box-shadow: 0 18px 40px rgba(0, 0, 0, 0.55);
}
.pills {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  padding-bottom: 4px;
}
.pill {
  display: inline-flex;
  align-items: center;
  gap: 7px;
  max-width: 100%;
  padding: 6px 8px 6px 12px;
  border-radius: 999px;
  border: 1px solid var(--line);
  background: color-mix(in srgb, var(--accent) 8%, transparent);
  color: var(--text);
  font-size: 14px;
  cursor: pointer;
}
.pill .words {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.x {
  display: grid;
  place-items: center;
  width: 20px;
  height: 20px;
  border: 0;
  border-radius: 50%;
  background: transparent;
  color: var(--text-dim);
  cursor: pointer;
}
.x:hover {
  background: var(--hover);
  color: var(--text);
}
.card {
  display: flex;
  align-items: center;
  gap: 12px;
  padding: 9px 12px;
  border-radius: 10px;
  border: 1px solid transparent;
  background: color-mix(in srgb, var(--accent) 4%, transparent);
  cursor: pointer;
}
.card:hover {
  background: var(--hover);
}
.pill.on,
.card.on {
  border-color: #7ee0b8;
  background: rgba(126, 224, 184, 0.1);
  box-shadow: 0 0 12px rgba(126, 224, 184, 0.35);
}
.letter {
  flex-shrink: 0;
  display: grid;
  place-items: center;
  width: 32px;
  height: 32px;
  border-radius: 50%;
  background: var(--text);
  color: #0b1636;
  font-weight: 700;
}
.text {
  min-width: 0;
  flex: 1;
  display: flex;
  flex-direction: column;
}
.title,
.sub {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.title {
  font-size: 14px;
  color: var(--text);
}
.sub {
  font-size: 12px;
  color: var(--text-dim);
}
.star {
  flex-shrink: 0;
  color: #ffd166;
}
</style>
