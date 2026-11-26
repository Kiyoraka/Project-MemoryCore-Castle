<script setup>
// Classic: the title bar ("Scriptorium · <editor>"), a ribbon of tab names and icon buttons, the page in
// the middle and the chat on the right (the pick scriptorium-scroll-1). The formatting buttons are
// placeholders until each editor lands; New, Open (with the recent files beside it), Save, Save as, Undo
// and Redo work now.
import { ref, watch } from 'vue'
import { EDITORS, ribbonTabs } from '../room.js'
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
  // the recent files: Studio lists them; Classic opens them from the menu beside Open
  recent: { type: Array, default: () => [] },
})
const emit = defineEmits(['editor', 'new', 'open', 'save', 'saveAs', 'undo', 'redo', 'recent'])

// the recent files' small menu
const menu = ref(false)
function pick(path) {
  menu.value = false
  emit('recent', path)
}
function leaveMenu(e) {
  if (!e.currentTarget.contains(e.relatedTarget)) menu.value = false
}

const tab = ref('Home')
watch(
  () => props.editor,
  () => (tab.value = 'Home'),
)

const later = (what) => `${what} - arrives with ${EDITORS.find((e) => e.id === props.editor)?.label ?? 'this editor'}`
const FORMAT = [
  [
    { name: 'bold', label: 'Bold' },
    { name: 'italic', label: 'Italic' },
    { name: 'underline', label: 'Underline' },
  ],
  [
    { name: 'bullets', label: 'Bullets' },
    { name: 'numbers', label: 'Numbering' },
  ],
  [
    { name: 'alignLeft', label: 'Align left' },
    { name: 'alignCenter', label: 'Centre' },
    { name: 'alignRight', label: 'Align right' },
  ],
  [
    { name: 'image', label: 'Picture' },
    { name: 'link', label: 'Link' },
    { name: 'table', label: 'Table' },
  ],
]
</script>

<template>
  <div class="classic">
    <header class="titlebar">
      <span class="mark" v-html="icon('spark', 20)"></span>
      <strong class="name">{{ title.split(' · ')[0] }}</strong>
      <span class="sep" aria-hidden="true"></span>
      <span class="editor-name">{{ title.split(' · ')[1] }}</span>
      <span v-if="file" class="file" :title="file">{{ file }}</span>
      <nav class="editors" aria-label="Editors">
        <button v-for="e in EDITORS" :key="e.id" :class="{ on: e.id === editor }" :aria-pressed="e.id === editor" :title="e.what" @click="emit('editor', e.id)">{{ e.label }}</button>
      </nav>
      <span v-if="badge" class="badge">{{ badge }}</span>
    </header>

    <nav class="tabs" role="tablist" aria-label="Ribbon">
      <button v-for="t in ribbonTabs(editor)" :key="t" role="tab" :aria-selected="t === tab" :class="{ on: t === tab }" @click="tab = t">{{ t }}</button>
    </nav>

    <div class="ribbon" role="toolbar" :aria-label="`${tab} tools`">
      <div class="group">
        <button title="New" :disabled="busy" @click="emit('new')" v-html="icon('new', 20)"></button>
        <button title="Open (Ctrl+O)" :disabled="busy" @click="emit('open')" v-html="icon('open', 20)"></button>
        <span class="recent-menu" @focusout="leaveMenu" @keydown.esc.stop="menu = false">
          <button class="narrow" title="Recent files" aria-haspopup="menu" :aria-expanded="menu" :disabled="busy" @click="menu = !menu">▾</button>
          <ul v-if="menu" class="menu" role="menu" aria-label="Recent files">
            <li v-for="f in recent.slice(0, 10)" :key="f.path" role="none">
              <button role="menuitem" :title="f.path" @click="pick(f.path)"><span v-html="icon(f.editor, 16)"></span><span class="menu-name">{{ f.name }}</span></button>
            </li>
            <li v-if="!recent.length" class="menu-empty" role="none">No recent files yet.</li>
          </ul>
        </span>
        <button title="Save (Ctrl+S)" :disabled="busy" @click="emit('save')" v-html="icon('save', 20)"></button>
        <button title="Save as (Ctrl+Shift+S)" :disabled="busy" @click="emit('saveAs')" v-html="icon('saveAs', 20)"></button>
      </div>
      <div class="group">
        <button title="Undo (Ctrl+Z)" :disabled="busy || !canUndo" @click="emit('undo')" v-html="icon('undo', 20)"></button>
        <button title="Redo (Ctrl+Y)" :disabled="busy || !canRedo" @click="emit('redo')" v-html="icon('redo', 20)"></button>
      </div>
      <div class="group">
        <span class="font" :title="later('Fonts')" aria-disabled="true"><span class="bar"></span><span class="chev">▾</span></span>
      </div>
      <div v-for="(g, i) in FORMAT" :key="i" class="group">
        <button v-for="b in g" :key="b.name" disabled :title="later(b.label)" :aria-label="b.label" v-html="icon(b.name, 20)"></button>
      </div>
      <div class="group">
        <button disabled title="Ask in the chat beside the page" aria-label="Ask in the chat" class="spark" v-html="icon('spark', 20)"></button>
      </div>
    </div>

    <div class="body">
      <nav class="rail" aria-label="Editors">
        <button v-for="e in EDITORS" :key="e.id" :class="{ on: e.id === editor }" :title="`${e.label} - ${e.what}`" :aria-label="e.label" @click="emit('editor', e.id)" v-html="icon(e.id, 20)"></button>
      </nav>
      <main class="desk">
        <slot name="page" />
        <div class="strip-slot"><slot name="strip" /></div>
      </main>
      <aside class="chat" aria-label="Chat">
        <slot name="chat" />
      </aside>
    </div>
    <p v-if="status" class="status" role="status">{{ status }}</p>
  </div>
</template>

<style scoped>
.classic {
  display: grid;
  grid-template-rows: auto auto auto 1fr auto;
  height: 100%;
  min-height: 0;
  color: var(--scr-text);
}
.titlebar {
  display: flex;
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
.sep {
  width: 1px;
  height: 18px;
  background: var(--scr-line);
}
.editor-name {
  color: var(--scr-muted);
}
.file {
  max-width: 260px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-size: 13px;
}
.recent-menu {
  position: relative;
  display: inline-flex;
}
.ribbon button.narrow {
  width: 18px;
  margin-left: -2px;
  font-size: 11px;
}
.menu {
  position: absolute;
  top: calc(100% + 6px);
  left: -40px;
  z-index: 20;
  min-width: 240px;
  max-width: 360px;
  margin: 0;
  padding: 6px;
  border: 1px solid var(--scr-line);
  border-radius: 12px;
  background: rgba(16, 13, 44, 0.96);
  box-shadow: 0 10px 30px rgba(0, 0, 0, 0.45);
  list-style: none;
}
.ribbon .menu button {
  display: flex;
  align-items: center;
  justify-content: flex-start;
  gap: 8px;
  width: 100%;
  height: auto;
  padding: 7px 8px;
  font: inherit;
  font-size: 13px;
  text-align: left;
}
.menu button > span:first-child {
  display: inline-flex;
  color: var(--scr-muted);
}
.menu-name {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.menu-empty {
  padding: 8px;
  color: var(--scr-muted);
  font-size: 13px;
}
.editors {
  display: flex;
  gap: 4px;
  margin-left: 18px;
}
.editors button,
.tabs button {
  padding: 4px 12px;
  border: 0;
  border-radius: 999px;
  background: transparent;
  color: var(--scr-muted);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}
.editors button:hover,
.tabs button:hover {
  color: var(--scr-text);
  background: var(--scr-hover);
}
.editors button.on,
.tabs button.on {
  color: var(--scr-text);
  background: var(--scr-chip);
}
.badge {
  margin-left: auto;
  padding: 3px 10px;
  border: 1px solid var(--scr-accent);
  border-radius: 999px;
  color: var(--scr-accent);
  font-size: 12px;
  white-space: nowrap;
}
.tabs {
  display: flex;
  gap: 6px;
  padding: 6px 16px;
}
.ribbon {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 4px;
  padding: 8px 16px;
  border-top: 1px solid var(--scr-line);
  border-bottom: 1px solid var(--scr-line);
  background: var(--scr-panel);
}
.group {
  display: flex;
  align-items: center;
  gap: 2px;
  padding-right: 10px;
  margin-right: 6px;
  border-right: 1px solid var(--scr-line);
}
.group:last-child {
  border-right: 0;
}
.ribbon button {
  display: inline-grid;
  place-items: center;
  width: 34px;
  height: 34px;
  padding: 0;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--scr-text);
  cursor: pointer;
}
.ribbon button:hover:not(:disabled) {
  background: var(--scr-hover);
}
.ribbon button:disabled {
  opacity: 0.38;
  cursor: default;
}
.ribbon button.spark {
  color: var(--scr-accent);
}
.font {
  display: inline-flex;
  align-items: center;
  gap: 18px;
  height: 30px;
  padding: 0 10px;
  border-radius: 8px;
  background: var(--scr-chip);
  opacity: 0.5;
}
.font .bar {
  width: 90px;
  height: 6px;
  border-radius: 3px;
  background: var(--scr-muted);
}
.font .chev {
  font-size: 11px;
}
.body {
  display: grid;
  grid-template-columns: 56px minmax(0, 1fr) minmax(300px, 30%);
  min-height: 0;
}
.rail {
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 8px;
  padding: 12px 0;
  border-right: 1px solid var(--scr-line);
}
.rail button {
  display: inline-grid;
  place-items: center;
  width: 38px;
  height: 38px;
  border: 0;
  border-radius: 10px;
  background: transparent;
  color: var(--scr-muted);
  cursor: pointer;
}
.rail button:hover {
  color: var(--scr-text);
  background: var(--scr-hover);
}
.rail button.on {
  color: var(--scr-text);
  background: var(--scr-chip);
}
.desk {
  position: relative;
  min-width: 0;
  min-height: 0;
  background: var(--scr-desk);
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
.chat {
  position: relative;
  min-width: 0;
  min-height: 0;
  margin: 10px;
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
@media (max-width: 900px) {
  .body {
    grid-template-columns: 48px minmax(0, 1fr);
    grid-template-rows: 1fr 40%;
  }
  .chat {
    grid-column: 1 / -1;
  }
}
</style>
