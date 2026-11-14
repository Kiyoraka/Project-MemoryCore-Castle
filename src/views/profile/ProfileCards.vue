<script setup>
import { audienceOf } from './profile.js'

// PROFILE, the Cards look (design profile-tab-1): the buttons on top, the waiting row in gold, then
// numbered cards with a small sign for who hears each trait. Draws only; ProfileView decides.
defineProps({
  companion: { type: String, default: 'Your companion' },
  lived: { type: Array, default: () => [] },
  waiting: { type: Array, default: () => [] },
  busy: { type: String, default: '' },
})
const emit = defineEmits(['open', 'keep', 'edit', 'no', 'scan', 'write'])
</script>

<template>
  <div class="cards-look">
    <h4>{{ companion.toUpperCase() }}'S HEART</h4>
    <div class="actions">
      <button class="primary" :disabled="!!busy" @click="emit('scan')">✦ {{ busy === 'diary' ? 'Reading…' : 'Scan my diary' }}</button>
      <button :disabled="!!busy" @click="emit('write')">✎ Write a trait</button>
    </div>

    <section v-if="waiting.length" class="waiting" aria-label="Waiting for your yes">
      <p class="title">✦ Waiting for your yes</p>
      <div v-for="w in waiting" :key="w.key" class="wait">
        <button class="what" :title="w.line" @click="emit('open', w)">
          <span class="name">{{ w.name }}</span>
          <span class="line">{{ w.line }}</span>
        </button>
        <div class="row">
          <button class="primary small" :disabled="!!busy" @click="emit('keep', w)">Keep</button>
          <button class="small" @click="emit('edit', w)">Edit</button>
          <button class="small" @click="emit('no', w)">No</button>
        </div>
      </div>
    </section>

    <p v-if="!lived.length && !waiting.length" class="empty">{{ companion }} has no traits yet. Scan your diary, or write one in your own words.</p>

    <div class="grid">
      <button v-for="t in lived" :key="t.number" class="card" :title="t.line" @click="emit('open', t)">
        <span class="n">{{ t.number }}</span>
        <span class="name">{{ t.name }}</span>
        <span class="who" :class="t.audience" :title="audienceOf(t.audience).label" :aria-label="audienceOf(t.audience).label">
          <svg v-if="t.audience === 'owner'" viewBox="0 0 24 24" aria-hidden="true"><path d="M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10z" /></svg>
          <svg v-else-if="t.audience === 'never'" viewBox="0 0 24 24" aria-hidden="true"><path d="M3 10c2.5 3 5.6 4.5 9 4.5S18.5 13 21 10M6 13.5 4.5 16M12 14.5V17M18 13.5l1.5 2.5" /></svg>
          <svg v-else viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="8.5" /><path d="M3.5 12h17M12 3.5c2.5 2.6 2.5 14.4 0 17M12 3.5c-2.5 2.6-2.5 14.4 0 17" /></svg>
        </span>
      </button>
    </div>
  </div>
</template>

<style scoped>
.cards-look {
  display: flex;
  flex-direction: column;
  gap: 12px;
}
h4 {
  margin: 0;
  font-size: 10.5px;
  letter-spacing: 0.3em;
  color: var(--text-dim);
}
.actions {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 8px;
}
.waiting {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px 12px;
  border: 1px solid #e6c27a;
  border-radius: 12px;
  box-shadow: 0 0 12px rgba(230, 194, 122, 0.25);
}
.waiting .title {
  margin: 0;
  font-size: 13px;
  color: #e6c27a;
}
.wait {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
  padding: 8px 10px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: rgba(0, 0, 0, 0.2);
}
.what {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 2px;
  min-width: 0;
  padding: 0;
  border: 0;
  text-align: left;
}
.what .line {
  font-size: 12px;
  color: var(--text-dim);
}
.row {
  display: flex;
  gap: 6px;
}
.empty {
  margin: 0;
  font-size: 13px;
  color: var(--text-dim);
}
.grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  gap: 8px;
}
.card {
  position: relative;
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 8px;
  min-height: 84px;
  padding: 10px 12px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: rgba(0, 0, 0, 0.2);
  text-align: left;
}
.card:hover {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 14%, rgba(0, 0, 0, 0.2));
}
.n {
  display: grid;
  place-items: center;
  width: 24px;
  height: 24px;
  border-radius: 50%;
  background: color-mix(in srgb, var(--accent) 30%, transparent);
  font-size: 12px;
  font-weight: 600;
}
.name {
  font-size: 13.5px;
}
.who {
  position: absolute;
  right: 10px;
  bottom: 8px;
  width: 16px;
  height: 16px;
  color: var(--accent);
}
.who svg {
  width: 100%;
  height: 100%;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.6;
  stroke-linecap: round;
}
button {
  padding: 8px 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}
button.primary {
  border-color: var(--accent);
  background: var(--accent);
  color: #07142e;
}
button.small {
  padding: 4px 10px;
  font-size: 12px;
}
button:disabled {
  opacity: 0.45;
  cursor: default;
}
button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
</style>
