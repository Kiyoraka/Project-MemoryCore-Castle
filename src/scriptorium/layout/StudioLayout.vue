<script setup>
// Studio: an icon rail of the editors, the recent files, the page, an inspector on the right and the chat
// docked along the bottom (the pick scriptorium-scroll-3). The toolbar over the page shows one tab of the
// editor's registry (ribbon.js) at a time, and the inspector its panels; the rail, the files, Undo, Redo,
// Save, Export PDF and Print are the room's.
import { computed, ref, watch } from 'vue'
import { EDITORS, editorOf } from '../room.js'
import { pressed, tabGroups } from '../ribbon.js'
import { icon, iconOf } from '../icons.js'

const props = defineProps({
  title: { type: String, required: true },
  // the document's file name ("Report.docx", "Untitled"; a dot after it while unsaved)
  file: { type: String, default: '' },
  editor: { type: String, required: true },
  badge: { type: String, default: '' },
  canUndo: { type: Boolean, default: false },
  canRedo: { type: Boolean, default: false },
  busy: { type: Boolean, default: false },
  status: { type: String, default: '' },
  // [{ path, name, editor }]
  recent: { type: Array, default: () => [] },
  // the editor's registry (ribbon.js editorUi) and its state under the selection
  ui: { type: Object, default: () => ({ tabs: [{ name: 'Home', groups: [] }], panels: [], intents: {} }) },
  editState: { type: Object, default: () => ({}) },
  canExport: { type: Boolean, default: false },
  canPrint: { type: Boolean, default: false },
})
const emit = defineEmits(['editor', 'new', 'open', 'save', 'saveAs', 'undo', 'redo', 'recent', 'button', 'exportPdf', 'print', 'find'])

const current = computed(() => editorOf(props.editor))
const tab = ref('Home')
const inspector = ref('')
watch(
  () => props.editor,
  () => {
    tab.value = 'Home'
    inspector.value = ''
  },
)
const tabNames = computed(() => (props.ui?.tabs ?? []).map((t) => t.name))
const groups = computed(() => tabGroups(props.ui, tab.value))
const panels = computed(() => props.ui?.panels ?? [])
const panel = computed(() => panels.value.find((p) => p.id === inspector.value) ?? panels.value[0] ?? null)
const titleOf = (b) => (b.ready ? b.title : `${b.title} - comes to ${current.value.label} soon`)
</script>

<template>
  <div class="studio">
    <header class="titlebar">
      <span class="mark" v-html="icon('spark', 20)"></span>
      <strong class="name">{{ title.split(' · ')[0] }}</strong>
      <span class="centre"><span v-html="icon(current.id, 18)"></span>{{ current.label }}<span v-if="file" class="file" :title="file">{{ file }}</span></span>
      <span class="actions">
        <button title="Undo (Ctrl+Z)" :disabled="busy || !canUndo" @click="emit('undo')" v-html="icon('undo', 18)"></button>
        <button title="Redo (Ctrl+Y)" :disabled="busy || !canRedo" @click="emit('redo')" v-html="icon('redo', 18)"></button>
        <button title="Save (Ctrl+S)" :disabled="busy" @click="emit('save')" v-html="icon('save', 18)"></button>
        <button title="Save as (Ctrl+Shift+S)" :disabled="busy" @click="emit('saveAs')" v-html="icon('saveAs', 18)"></button>
        <button title="Export as PDF" :disabled="busy || !canExport" @click="emit('exportPdf')" v-html="icon('exportPdf', 18)"></button>
        <button title="Print (Ctrl+P)" :disabled="busy || !canPrint" @click="emit('print')" v-html="icon('print', 18)"></button>
        <span v-if="badge" class="badge">{{ badge }}</span>
      </span>
    </header>

    <div class="body">
      <nav class="rail" aria-label="Editors">
        <button v-for="e in EDITORS" :key="e.id" :class="{ on: e.id === editor }" :title="`${e.label} - ${e.what}`" :aria-label="e.label" :aria-pressed="e.id === editor" @click="emit('editor', e.id)" v-html="icon(e.id, 22)"></button>
      </nav>

      <aside class="files" aria-label="Files">
        <div class="file-actions">
          <button :disabled="busy" @click="emit('new')"><span v-html="icon('new', 16)"></span>New</button>
          <button :disabled="busy" title="Open (Ctrl+O)" @click="emit('open')"><span v-html="icon('open', 16)"></span>Open…</button>
        </div>
        <h3><span v-html="icon('clock', 16)"></span>Recent</h3>
        <ul v-if="recent.length" class="recent">
          <li v-for="f in recent" :key="f.path">
            <button :title="f.path" :disabled="busy" @click="emit('recent', f.path)"><span v-html="icon(f.editor, 16)"></span><span class="file-name">{{ f.name }}</span></button>
          </li>
        </ul>
        <p v-else class="empty">No recent files yet.</p>
      </aside>

      <main class="desk">
        <div class="toolbar" role="toolbar" :aria-label="`${tab} tools`">
          <span v-if="tabNames.length > 1" class="tab-pick" role="tablist" aria-label="Tools">
            <button v-for="t in tabNames" :key="t" role="tab" class="tab" :class="{ on: t === tab }" :aria-selected="t === tab" @click="tab = t">{{ t }}</button>
          </span>
          <button title="Find (Ctrl+F)" aria-label="Find" @click="emit('find')" v-html="icon('find', 18)"></button>
          <template v-for="(g, i) in groups" :key="`${tab}-${i}`">
            <span class="tool-sep" aria-hidden="true"></span>
            <button
              v-for="(b, j) in g.buttons"
              :key="j"
              :class="{ on: pressed(editState, b.stateKey) }"
              :aria-pressed="b.stateKey ? pressed(editState, b.stateKey) : undefined"
              :aria-label="b.title"
              :title="titleOf(b)"
              :disabled="busy || !b.ready"
              @click="emit('button', b)"
              v-html="iconOf(b.icon, 18)"
            ></button>
          </template>
        </div>
        <div class="page-slot"><slot name="page" /></div>
        <div class="strip-slot"><slot name="strip" /></div>
      </main>

      <aside class="inspector" aria-label="Inspector">
        <nav v-if="panels.length > 1" class="inspector-tabs" role="tablist">
          <button v-for="p in panels" :key="p.id" role="tab" :aria-selected="panel?.id === p.id" :class="{ on: panel?.id === p.id }" :title="p.label" @click="inspector = p.id" v-html="iconOf(p.icon || 'sliders', 20)"></button>
        </nav>
        <template v-if="panel">
          <section v-for="(g, i) in panel.groups" :key="`${panel.id}-${i}`" class="card">
            <div class="card-body">
              <span v-if="g.label" class="card-label">{{ g.label }}</span>
              <div class="card-tools">
                <button
                  v-for="(b, j) in g.buttons"
                  :key="j"
                  :class="{ on: pressed(editState, b.stateKey) }"
                  :aria-pressed="b.stateKey ? pressed(editState, b.stateKey) : undefined"
                  :aria-label="b.title"
                  :title="titleOf(b)"
                  :disabled="busy || !b.ready"
                  @click="emit('button', b)"
                  v-html="iconOf(b.icon, 18)"
                ></button>
              </div>
            </div>
          </section>
        </template>
        <p v-else class="hint">Nothing to set here yet for {{ current.label }}.</p>
      </aside>
    </div>

    <section class="dock" aria-label="Chat">
      <slot name="chat" />
    </section>
    <p v-if="status" class="status" role="status">{{ status }}</p>
  </div>
</template>

<style scoped>
.studio {
  display: grid;
  grid-template-rows: auto minmax(0, 1fr) minmax(220px, 32%) auto;
  height: 100%;
  min-height: 0;
  color: var(--scr-text);
}
.titlebar {
  display: grid;
  grid-template-columns: auto auto 1fr auto;
  align-items: center;
  gap: 10px;
  padding: 10px 16px;
  border-bottom: 1px solid var(--scr-line);
}
.mark {
  display: inline-flex;
  color: var(--scr-accent);
}
.name {
  font-weight: 600;
}
.centre {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  font-size: 16px;
}
.centre > span {
  display: inline-flex;
}
.centre > .file {
  display: inline-block;
  max-width: 280px;
  overflow: hidden;
  color: var(--scr-muted);
  font-size: 13px;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.actions {
  display: inline-flex;
  align-items: center;
  gap: 4px;
}
.actions button,
.toolbar button {
  display: inline-grid;
  place-items: center;
  width: 32px;
  height: 32px;
  padding: 0;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--scr-text);
  cursor: pointer;
}
.actions button:hover:not(:disabled),
.toolbar button:hover:not(:disabled) {
  background: var(--scr-hover);
}
.actions button:disabled,
.toolbar button:disabled {
  opacity: 0.38;
  cursor: default;
}
.badge {
  margin-left: 8px;
  padding: 3px 10px;
  border: 1px solid var(--scr-accent);
  border-radius: 999px;
  color: var(--scr-accent);
  font-size: 12px;
  white-space: nowrap;
}
.body {
  display: grid;
  grid-template-columns: 64px 220px minmax(0, 1fr) 280px;
  min-height: 0;
}
.rail {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  padding: 14px 0;
  border-right: 1px solid var(--scr-line);
}
.rail button {
  display: inline-grid;
  place-items: center;
  width: 44px;
  height: 44px;
  border: 0;
  border-radius: 12px;
  background: transparent;
  color: var(--scr-muted);
  cursor: pointer;
}
.rail button:hover {
  color: var(--scr-text);
  background: var(--scr-hover);
}
.rail button.on {
  color: #fff;
  background: var(--scr-accent);
}
.files {
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
  padding: 14px 12px;
  border-right: 1px solid var(--scr-line);
  overflow: auto;
}
.file-actions {
  display: flex;
  gap: 6px;
}
.file-actions button {
  display: inline-flex;
  flex: 1;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 7px 8px;
  border: 1px solid var(--scr-line);
  border-radius: 10px;
  background: var(--scr-chip);
  color: var(--scr-text);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}
.file-actions button span {
  display: inline-flex;
}
.files h3 {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 10px 4px 2px;
  color: var(--scr-muted);
  font-size: 12px;
  font-weight: 600;
  letter-spacing: 0.04em;
  text-transform: uppercase;
}
.files h3 span {
  display: inline-flex;
}
.recent {
  margin: 0;
  padding: 0;
  list-style: none;
}
.recent button {
  display: flex;
  align-items: center;
  gap: 8px;
  width: 100%;
  padding: 7px 8px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--scr-text);
  font: inherit;
  font-size: 13px;
  text-align: left;
  cursor: pointer;
}
.recent button:hover {
  background: var(--scr-hover);
}
.recent button > span:first-child {
  display: inline-flex;
  color: var(--scr-muted);
}
.file-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.empty,
.hint {
  margin: 4px;
  color: var(--scr-muted);
  font-size: 13px;
}
.desk {
  position: relative;
  display: grid;
  grid-template-rows: auto minmax(0, 1fr);
  min-width: 0;
  min-height: 0;
  background: var(--scr-desk);
}
.toolbar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 2px;
  padding: 6px 12px;
  border-bottom: 1px solid var(--scr-line);
}
.toolbar button.on,
.card-tools button.on {
  background: var(--scr-chip);
}
.tab-pick {
  display: inline-flex;
  gap: 2px;
  margin-right: 4px;
}
.toolbar .tab-pick button.tab {
  width: auto;
  padding: 0 10px;
  border-radius: 999px;
  color: var(--scr-muted);
  font: inherit;
  font-size: 12px;
}
.toolbar .tab-pick button.tab.on {
  color: var(--scr-text);
}
.tool-sep {
  width: 1px;
  height: 20px;
  margin: 0 6px;
  background: var(--scr-line);
}
.page-slot {
  position: relative;
  min-height: 0;
}
.strip-slot {
  position: absolute;
  left: 16px;
  right: 16px;
  bottom: 12px;
  pointer-events: none;
}
.strip-slot > :deep(*) {
  pointer-events: auto;
}
.inspector {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
  padding: 12px;
  border-left: 1px solid var(--scr-line);
  overflow: auto;
}
.inspector-tabs {
  display: grid;
  grid-template-columns: repeat(4, 1fr);
  gap: 4px;
  padding: 4px;
  border: 1px solid var(--scr-line);
  border-radius: 12px;
}
.inspector-tabs button {
  display: inline-grid;
  place-items: center;
  height: 36px;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--scr-muted);
  cursor: pointer;
}
.inspector-tabs button.on {
  color: var(--scr-text);
  background: var(--scr-chip);
}
.card {
  display: flex;
  gap: 12px;
  padding: 12px;
  border: 1px solid var(--scr-line);
  border-radius: 12px;
  background: var(--scr-panel);
}
.card-body {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 10px;
}
.card-label {
  font-size: 13px;
}
.card-tools {
  display: flex;
  flex-wrap: wrap;
  gap: 2px;
}
.card-tools button {
  display: inline-grid;
  place-items: center;
  width: 32px;
  height: 32px;
  padding: 0;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--scr-text);
  cursor: pointer;
}
.card-tools button:hover:not(:disabled) {
  background: var(--scr-hover);
}
.card-tools button:disabled {
  opacity: 0.38;
  cursor: default;
}
.dock {
  position: relative;
  min-height: 0;
  margin: 8px 10px 10px;
  border: 1px solid var(--scr-line);
  border-radius: 14px;
  background: var(--scr-panel);
  overflow: hidden;
}
.status {
  margin: 0;
  padding: 6px 16px;
  border-top: 1px solid var(--scr-line);
  color: var(--scr-muted);
  font-size: 13px;
}
@media (max-width: 1100px) {
  .body {
    grid-template-columns: 56px 180px minmax(0, 1fr);
  }
  .inspector {
    display: none;
  }
}
</style>
