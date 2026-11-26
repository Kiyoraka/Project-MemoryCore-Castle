<script setup>
// Folio's bookmarks list: the document's outline (read { outline: true }) as rows, nested by depth; a
// click on a row asks for the bus command that turns to its page. Host-agnostic: the room gives the
// outline and runs the command it emits. (It waits for the room to mount an editor's own panel.)
import { computed } from 'vue'
import { bookmarkCommand, outlineRows } from './index.js'

const props = defineProps({
  // read { outline: true }'s answer, or its `outline` array
  outline: { type: [Array, Object], default: () => [] },
  // the page on show, to mark the bookmarks that lead there
  page: { type: Number, default: -1 },
})
const emit = defineEmits(['command'])

const rows = computed(() => outlineRows(props.outline))

function go(row) {
  const c = bookmarkCommand(row)
  if (c) emit('command', c)
}
</script>

<template>
  <nav class="folio-bookmarks" aria-label="Bookmarks">
    <p v-if="!rows.length" class="empty">This document has no bookmarks.</p>
    <button
      v-for="r in rows"
      :key="r.index"
      type="button"
      class="row"
      :class="{ here: r.page === page, nowhere: r.page === null }"
      :style="{ paddingLeft: `${8 + r.depth * 14}px` }"
      :disabled="r.page === null"
      :title="r.page === null ? 'This bookmark leads to no page' : `Page ${r.page + 1}`"
      @click="go(r)"
    >
      <span class="title">{{ r.title || 'Untitled' }}</span>
      <span v-if="r.page !== null" class="page">{{ r.page + 1 }}</span>
    </button>
  </nav>
</template>

<style scoped>
.folio-bookmarks {
  display: flex;
  flex-direction: column;
  gap: 2px;
}
.row {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 5px 8px;
  border: 0;
  border-radius: 6px;
  background: transparent;
  color: inherit;
  font: inherit;
  text-align: left;
  cursor: pointer;
}
.row:hover:not(:disabled) {
  background: color-mix(in srgb, currentColor 10%, transparent);
}
.row.here {
  background: color-mix(in srgb, var(--scr-accent, #8b7bff) 22%, transparent);
}
.row:disabled {
  opacity: 0.55;
  cursor: default;
}
.title {
  flex: 1;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.page {
  opacity: 0.6;
  font-variant-numeric: tabular-nums;
}
.empty {
  margin: 4px 8px;
  opacity: 0.7;
}
</style>
