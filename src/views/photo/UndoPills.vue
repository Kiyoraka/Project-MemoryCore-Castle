<script setup>
// The strip under the photo: one pill per change, each with its own undo.
import { nextTick, ref, watch } from 'vue'

const props = defineProps({
  pills: { type: Array, default: () => [] },
})
const emit = defineEmits(['undo'])

const row = ref(null)

// keep the newest pill in view
watch(
  () => props.pills.length,
  async (now, before) => {
    if (now <= (before ?? 0)) return
    await nextTick()
    if (row.value) row.value.scrollLeft = row.value.scrollWidth
  },
)
</script>

<template>
  <div v-if="pills.length" ref="row" class="pills">
    <div v-for="pill in pills" :key="pill.id" class="pill">
      <span class="label">{{ pill.label }}</span>
      <button type="button" class="undo" title="Undo this" aria-label="Undo this" @click="emit('undo', pill)">
        <svg viewBox="0 0 24 24" aria-hidden="true">
          <path d="M9 14 4 9l5-5" />
          <path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11" />
        </svg>
      </button>
    </div>
  </div>
</template>

<style scoped>
.pills {
  display: flex;
  gap: 10px;
  padding: 10px 14px;
  overflow-x: auto;
  overflow-y: hidden;
  border: 1px solid var(--line, rgba(190, 170, 255, 0.3));
  border-radius: 14px;
  background: var(--panel-background, rgba(24, 19, 66, 0.72));
  backdrop-filter: blur(var(--glass-blur, 8px));
  scrollbar-width: var(--scrollbar-width);
  scrollbar-color: var(--scrollbar-thumb) var(--scrollbar-track);
}

.pill {
  display: flex;
  flex: 0 0 auto;
  align-items: center;
  gap: 10px;
  padding: 7px 8px 7px 16px;
  border-radius: 999px;
  background: color-mix(in srgb, var(--accent, #8a5cf0) 22%, transparent);
  color: var(--text, #efe8ff);
  font-size: 14px;
  white-space: nowrap;
}

.undo {
  display: grid;
  place-items: center;
  width: 26px;
  height: 26px;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: transparent;
  color: var(--text-dim, #c9bdf0);
  cursor: pointer;
}

.undo:hover {
  background: color-mix(in srgb, var(--accent, #8a5cf0) 40%, transparent);
  color: #fff;
}

.undo svg {
  width: 17px;
  height: 17px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.8;
  stroke-linecap: round;
  stroke-linejoin: round;
}
</style>
