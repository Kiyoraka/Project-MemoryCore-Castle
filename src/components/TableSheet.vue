<script setup>
import { computed, nextTick, onMounted, ref } from 'vue'
import ModalSheet from './ModalSheet.vue'
import {
  addColumn,
  addRow,
  fromTable,
  message,
  removeColumn,
  removeRow,
  renameColumn,
  selectAll,
  selectedCount,
  setCell,
  setCheck,
  setTickName,
  toggleSelected,
  toggleTicked,
} from './chatTable.js'

// A table from a chat message, in its own window: edit it (cells, rows, columns, a column of ticks),
// pick rows to send back, or tick rows as answers ("tested"), and add a few words. Send puts it in
// the chat box as markdown; the member still presses send. The message itself never changes.
const props = defineProps({
  table: { type: Object, required: true }, // { head, rows } from parseBlocks
})
const emit = defineEmits(['send', 'close'])

const model = ref(fromTable(props.table.head, props.table.rows))
const context = ref('')
const contextBox = ref(null)
const picked = computed(() => selectedCount(model.value))
const allPicked = computed(() => model.value.rows.length > 0 && picked.value === model.value.rows.length)
const tickedCount = computed(() => model.value.rows.filter((r) => r.ticked).length)

const edit = (fn, ...args) => {
  model.value = fn(model.value, ...args)
}

function send(rows) {
  const text = message(model.value, context.value, rows)
  if (text) emit('send', text)
}

async function addRowAndFocus() {
  edit(addRow)
  await nextTick()
  document.querySelector('.table-sheet tbody tr:last-child input.cell')?.focus()
}

onMounted(() => contextBox.value?.focus({ preventScroll: true }))
</script>

<template>
  <ModalSheet tag="TABLE" title="Reply to this table" @close="emit('close')">
    <div class="table-sheet">
      <p class="hint">Edit, pick rows to send, or tick rows as your answer. The message in the chat stays as it was.</p>

      <div class="grid-wrap">
        <table>
          <thead>
            <tr>
              <th class="side" scope="col">
                <label class="pick" :title="allPicked ? 'Unpick every row' : 'Pick every row'">
                  <input type="checkbox" :checked="allPicked" :indeterminate.prop="picked > 0 && !allPicked" aria-label="Pick every row" @change="edit(selectAll)" />
                </label>
              </th>
              <th v-for="(col, c) in model.columns" :key="c" scope="col">
                <input class="head" :value="col.name" :aria-label="`Column ${c + 1} name`" @input="edit(renameColumn, c, $event.target.value)" />
                <div class="col-tools">
                  <button type="button" class="chip" :class="{ on: col.check }" :aria-pressed="col.check" :title="col.check ? 'Back to text' : 'Make this a column of ticks'" @click="edit(setCheck, c, !col.check)">☑ Ticks</button>
                  <button type="button" class="icon" :disabled="model.columns.length <= 1" :aria-label="`Remove column ${col.name || c + 1}`" title="Remove column" @click="edit(removeColumn, c)">×</button>
                </div>
              </th>
              <th class="tick-head" scope="col">
                <input class="head" :value="model.tickName" aria-label="Name of the tick column" title="Ticked rows are sent with this column" @input="edit(setTickName, $event.target.value)" />
              </th>
              <th class="side" scope="col">
                <button type="button" class="icon add" aria-label="Add a column" title="Add a column" @click="edit(addColumn, 'New')">+</button>
              </th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(row, r) in model.rows" :key="r" :class="{ picked: row.selected, ticked: row.ticked }">
              <td class="side">
                <input type="checkbox" class="pick" :checked="row.selected" :aria-label="`Pick row ${r + 1}`" @change="edit(toggleSelected, r)" />
              </td>
              <td v-for="(cell, c) in row.cells" :key="c">
                <input v-if="model.columns[c].check" type="checkbox" class="tick-cell" :checked="cell === true" :aria-label="`${model.columns[c].name || 'Column ' + (c + 1)}, row ${r + 1}`" @change="edit(setCell, r, c, $event.target.checked)" />
                <input v-else class="cell" :value="cell" :aria-label="`${model.columns[c].name || 'Column ' + (c + 1)}, row ${r + 1}`" @input="edit(setCell, r, c, $event.target.value)" />
              </td>
              <td class="tick-col">
                <button type="button" class="tick" :class="{ on: row.ticked }" :aria-pressed="row.ticked" :aria-label="`${model.tickName || 'Done'}: row ${r + 1}`" @click="edit(toggleTicked, r)">{{ row.ticked ? '✓' : '○' }}</button>
              </td>
              <td class="side">
                <button type="button" class="icon" :aria-label="`Remove row ${r + 1}`" title="Remove row" @click="edit(removeRow, r)">×</button>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
      <button type="button" class="add-row" @click="addRowAndFocus">+ Row</button>

      <label class="context">
        <span>Your words with it</span>
        <textarea ref="contextBox" v-model="context" rows="3" placeholder="Say what you mean about this table - it goes in front of the table." />
      </label>

      <footer>
        <span class="dim">{{ tickedCount ? `${tickedCount} ticked` : '' }}{{ tickedCount && picked ? ' · ' : '' }}{{ picked ? `${picked} picked` : '' }}</span>
        <button type="button" class="ghost" @click="emit('close')">Cancel</button>
        <button type="button" class="ghost" :disabled="!picked" @click="send('selected')">Send {{ picked || '' }} picked row{{ picked === 1 ? '' : 's' }}</button>
        <button type="button" class="primary" @click="send('all')">Put table in chat</button>
      </footer>
    </div>
  </ModalSheet>
</template>

<style scoped>
.table-sheet {
  display: flex;
  flex-direction: column;
  gap: 12px;
  min-height: 100%;
}
.hint,
.dim {
  margin: 0;
  font-size: 12px;
  color: var(--text-dim);
}
.grid-wrap {
  overflow: auto;
  border: 1px solid var(--line);
  border-radius: 10px;
}
table {
  width: 100%;
  border-collapse: collapse;
  font-size: 13px;
}
th,
td {
  padding: 4px 6px;
  border-bottom: 1px solid var(--line);
  vertical-align: middle;
  text-align: left;
}
th {
  background: rgba(0, 0, 0, 0.12);
}
th.side,
td.side {
  width: 28px;
  text-align: center;
}
.tick-head,
.tick-col {
  width: 70px;
  text-align: center;
  border-left: 1px solid var(--line);
}
tr.picked td {
  background: var(--hover, rgba(200, 180, 255, 0.12));
}
tr.ticked .tick-col {
  color: var(--accent);
}
input.head,
input.cell {
  width: 100%;
  min-width: 80px;
  box-sizing: border-box;
  padding: 4px 6px;
  border: 1px solid transparent;
  border-radius: 6px;
  background: none;
  color: inherit;
  font: inherit;
}
input.head {
  font-weight: 600;
}
.tick-head input.head {
  min-width: 50px;
  text-align: center;
}
input.head:hover,
input.cell:hover {
  border-color: var(--line);
}
input.head:focus,
input.cell:focus {
  border-color: var(--accent);
  outline: none;
}
.col-tools {
  display: flex;
  gap: 4px;
  margin-top: 2px;
}
.chip,
.icon,
.tick,
.add-row,
footer button {
  border: 1px solid var(--line);
  border-radius: 6px;
  background: none;
  color: inherit;
  font: inherit;
  cursor: pointer;
}
.chip {
  padding: 1px 6px;
  font-size: 11px;
  color: var(--text-dim);
}
.chip.on {
  border-color: var(--accent);
  color: var(--accent);
}
.icon {
  width: 22px;
  height: 22px;
  padding: 0;
  border-color: transparent;
  color: var(--text-dim);
}
.icon:hover:not(:disabled),
.chip:hover,
.add-row:hover {
  background: var(--hover, rgba(200, 180, 255, 0.16));
}
.icon:disabled {
  opacity: 0.35;
  cursor: default;
}
.icon.add {
  font-size: 16px;
}
.tick {
  width: 28px;
  height: 24px;
  border-color: transparent;
  font-size: 15px;
  color: var(--text-dim);
}
.tick.on {
  color: var(--accent);
}
.add-row {
  align-self: flex-start;
  padding: 3px 10px;
  font-size: 12px;
}
.context {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 12px;
  color: var(--text-dim);
}
.context textarea {
  resize: vertical;
  padding: 8px 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.15);
  color: var(--text);
  font: inherit;
  font-size: 14px;
}
.context textarea:focus {
  border-color: var(--accent);
  outline: none;
}
footer {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-top: auto;
}
footer .dim {
  flex: 1;
}
footer button {
  padding: 6px 14px;
  font-size: 13px;
}
footer button:disabled {
  opacity: 0.4;
  cursor: default;
}
footer .primary {
  border-color: var(--accent);
  background: var(--accent);
  color: var(--bg, #000);
  font-weight: 600;
}
button:focus-visible,
input[type='checkbox']:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}
input[type='checkbox'] {
  accent-color: var(--accent);
  cursor: pointer;
}
</style>
