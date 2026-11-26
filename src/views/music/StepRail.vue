<script setup>
import { STEPS } from './musicActions.js'

// The six steps across the top (SEED to FINISH); the current one lit. A click is step.set.
defineProps({
  step: { type: String, required: true },
  // the step lit by her hand just now
  hers: { type: Boolean, default: false },
})
const emit = defineEmits(['set'])
</script>

<template>
  <nav class="rail" aria-label="The song's steps">
    <span class="line" aria-hidden="true"></span>
    <button
      v-for="(s, n) in STEPS"
      :key="s"
      class="dot"
      :class="{ on: s === step, done: STEPS.indexOf(step) > n, hers: hers && s === step }"
      :aria-current="s === step ? 'step' : undefined"
      @click="emit('set', s)"
    >
      <span class="num">{{ n + 1 }}</span>
      <span class="name">{{ s.toUpperCase() }}</span>
    </button>
  </nav>
</template>

<style scoped>
.rail {
  position: relative;
  display: flex;
  justify-content: space-between;
  align-items: flex-start;
  padding: 10px 28px 8px;
  border: 1px solid var(--line);
  border-radius: 16px;
  background: var(--panel-background);
  backdrop-filter: blur(var(--glass-blur));
}

.line {
  position: absolute;
  top: 25px;
  left: 50px;
  right: 50px;
  height: 2px;
  background: linear-gradient(90deg, color-mix(in srgb, var(--accent) 25%, transparent), color-mix(in srgb, var(--accent) 55%, transparent), color-mix(in srgb, var(--accent) 25%, transparent));
}

.dot {
  position: relative;
  z-index: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 4px;
  padding: 0;
  border: 0;
  background: none;
  color: var(--text-dim);
  font: inherit;
  cursor: pointer;
}

.num {
  display: grid;
  place-items: center;
  width: 30px;
  height: 30px;
  border: 1.5px solid color-mix(in srgb, var(--accent) 55%, transparent);
  border-radius: 50%;
  background: var(--panel-background);
  font-size: 14px;
  color: var(--text);
  transition: box-shadow 0.3s, background 0.3s;
}

.name {
  font-size: 11.5px;
  letter-spacing: 0.1em;
}

.dot.done .num {
  background: color-mix(in srgb, var(--accent) 35%, transparent);
}

.dot.on .num {
  border-color: var(--text);
  background: var(--accent);
  box-shadow: 0 0 0 4px color-mix(in srgb, var(--accent) 25%, transparent), 0 0 18px color-mix(in srgb, var(--accent) 80%, transparent);
}

.dot.on .name {
  color: var(--text);
}

.dot.hers .num {
  animation: hers 1.4s ease-out;
}

.dot:hover .num {
  box-shadow: 0 0 12px color-mix(in srgb, var(--accent) 55%, transparent);
}

@keyframes hers {
  0% { box-shadow: 0 0 0 0 rgba(255, 210, 122, 0.9); }
  100% { box-shadow: 0 0 0 14px rgba(255, 210, 122, 0); }
}
</style>
