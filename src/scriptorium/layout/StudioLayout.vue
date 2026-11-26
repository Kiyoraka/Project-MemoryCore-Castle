<script setup>
// Studio: an icon rail of the editors, the recent files, the page, an inspector on the right and the chat
// docked along the bottom (the pick scriptorium-scroll-3). The inspector's controls are placeholders until
// each editor lands; the rail, the files, Undo and Redo work now.
import { computed, ref } from 'vue'
import { EDITORS, editorOf } from '../room.js'
import { icon } from '../icons.js'

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
})
const emit = defineEmits(['editor', 'new', 'open', 'save', 'saveAs', 'undo', 'redo', 'recent'])

const current = computed(() => editorOf(props.editor))
const inspectorTabs = [
  { id: 'look', icon: 'sliders', label: 'Look' },
  { id: 'text', icon: 'text', label: 'Text' },
  { id: 'picture', icon: 'image', label: 'Pictures' },
  { id: 'layout', icon: 'table', label: 'Layout' },
]
const inspector = ref('look')
const later = (what) => `${what} - arrives with ${current.value.label}`
const SECTIONS = [
  { icon: 'text', label: 'Text' },
  { icon: 'paragraph', label: 'Paragraph' },
  { icon: 'palette', label: 'Colour' },
  { icon: 'image', label: 'Picture' },
  { icon: 'frame', label: 'Frame' },
]
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
        <div class="toolbar" role="toolbar" aria-label="Format">
          <button v-for="b in ['bold', 'italic', 'underline', 'alignLeft', 'alignCenter', 'alignRight', 'bullets', 'numbers', 'link', 'image']" :key="b" disabled :title="later(b)" :aria-label="b" v-html="icon(b, 18)"></button>
        </div>
        <div class="page-slot"><slot name="page" /></div>
        <div class="strip-slot"><slot name="strip" /></div>
      </main>

      <aside class="inspector" aria-label="Inspector">
        <nav class="inspector-tabs" role="tablist">
          <button v-for="t in inspectorTabs" :key="t.id" role="tab" :aria-selected="inspector === t.id" :class="{ on: inspector === t.id }" :title="t.label" @click="inspector = t.id" v-html="icon(t.icon, 20)"></button>
        </nav>
        <section v-for="s in SECTIONS" :key="s.label" class="card" :title="later(s.label)">
          <span class="card-icon" v-html="icon(s.icon, 18)"></span>
          <div class="card-body">
            <span class="card-label">{{ s.label }}</span>
            <span class="slider" aria-hidden="true"><span></span></span>
          </div>
        </section>
        <p class="hint">The inspector fills in as each editor lands.</p>
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
  gap: 2px;
  padding: 6px 12px;
  border-bottom: 1px solid var(--scr-line);
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
  opacity: 0.6;
}
.card-icon {
  display: inline-flex;
  color: var(--scr-muted);
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
.slider {
  position: relative;
  height: 4px;
  border-radius: 2px;
  background: var(--scr-chip);
}
.slider span {
  position: absolute;
  inset: 0 45% 0 0;
  border-radius: 2px;
  background: var(--scr-accent);
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
