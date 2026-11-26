<script setup>
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'

// PHOTO's menu row v3: File, Edit, Image, Layer, Select, Filter, View, Window. Every item goes out as
// run(id); the view does the work. Shortcuts are shown as Photoshop has them; a ticked item shows its tick;
// an item with a list opens it to the side.
const props = defineProps({
  hasDoc: { type: Boolean, default: false },
  canUndo: { type: Boolean, default: false },
  canRedo: { type: Boolean, default: false },
  hasSelection: { type: Boolean, default: false },
  layerKind: { type: String, default: '' }, // the chosen layer's kind, '' for none
  hasMask: { type: Boolean, default: false },
  canMergeDown: { type: Boolean, default: false },
  clipped: { type: Boolean, default: false },
  hasGuides: { type: Boolean, default: false },
  checks: { type: Object, default: () => ({}) }, // item id -> ticked
  adjustments: { type: Array, default: () => [] }, // [{ type, label, keys? }]
  filters: { type: Array, default: () => [] }, // [{ id, label, group }]
  artboards: { type: Array, default: () => [] }, // [{ id, label }]
  panels: { type: Array, default: () => [] }, // [{ id, label }]
})
const emit = defineEmits(['run'])

const open = ref('')
const sub = ref('')
const bar = ref(null)

const menus = computed(() => {
  const doc = props.hasDoc
  const image = props.layerKind === 'image'
  const any = !!props.layerKind
  const adjust = props.adjustments.map((a) => ({ id: `adj-${a.type}`, label: `${a.label}…`, keys: a.keys, off: !doc }))
  const groups = []
  for (const f of props.filters) {
    let g = groups.find((x) => x.label === f.group)
    if (!g) groups.push((g = { id: `fgroup-${f.group}`, label: f.group, items: [] }))
    g.items.push({ id: `filter-${f.id}`, label: `${f.label}…`, off: !image })
  }
  return [
    {
      id: 'file',
      label: 'File',
      items: [
        { id: 'new', label: 'New…', keys: 'Ctrl+N' },
        { id: 'open', label: 'Open… (pictures, .psd)', keys: 'Ctrl+O' },
        { id: 'recent', label: 'Open recent…' },
        { sep: true },
        { id: 'save', label: 'Save', keys: 'Ctrl+S', off: !doc },
        { id: 'export-as', label: 'Export As…', keys: 'Alt+Shift+Ctrl+W', off: !doc },
        { id: 'export-png', label: 'Quick export as PNG…', off: !doc },
        { id: 'export-jpeg', label: 'Quick export as JPG…', off: !doc },
      ],
    },
    {
      id: 'edit',
      label: 'Edit',
      items: [
        { id: 'undo', label: 'Undo', keys: 'Ctrl+Z', off: !props.canUndo },
        { id: 'redo', label: 'Redo', keys: 'Ctrl+Y', off: !props.canRedo },
        { sep: true },
        { id: 'fill-colour', label: 'Fill with the primary colour', keys: 'Alt+Backspace', off: !image },
        { id: 'fill-content', label: 'Content-aware fill', keys: 'Shift+F5', off: !image || !props.hasSelection },
        { sep: true },
        { id: 'free-transform', label: 'Free transform', keys: 'Ctrl+T', off: !image },
      ],
    },
    {
      id: 'image',
      label: 'Image',
      items: [
        { id: 'adjustments', label: 'Adjustments', items: adjust, off: !doc },
        { sep: true },
        { id: 'artboards', label: 'New artboard', items: props.artboards.map((a) => ({ id: `artboard-${a.id}`, label: a.label, off: !doc })), off: !doc },
        { id: 'artboard-custom', label: 'New artboard (custom size)…', off: !doc },
        { sep: true },
        { id: 'crop', label: 'Crop', off: !doc },
        { id: 'rotate-left', label: 'Rotate 90° left', off: !doc },
        { id: 'rotate-right', label: 'Rotate 90° right', off: !doc },
        { id: 'flip-h', label: 'Flip horizontal', off: !doc },
        { id: 'flip-v', label: 'Flip vertical', off: !doc },
        { sep: true },
        { id: 'image-size', label: 'Image size…', keys: 'Ctrl+Alt+I', off: !doc },
        { id: 'canvas-size', label: 'Canvas size…', keys: 'Ctrl+Alt+C', off: !doc },
      ],
    },
    {
      id: 'layer',
      label: 'Layer',
      items: [
        { id: 'layer-new', label: 'New layer', keys: 'Ctrl+Shift+N', off: !doc },
        { id: 'layer-duplicate', label: 'Duplicate layer', keys: 'Ctrl+J', off: !any },
        { id: 'layer-delete', label: 'Delete layer', off: !any },
        { sep: true },
        { id: 'layer-group', label: 'New group', keys: 'Ctrl+G', off: !doc },
        { id: 'layer-ungroup', label: 'Ungroup', keys: 'Ctrl+Shift+G', off: props.layerKind !== 'group' },
        { id: 'layer-clip', label: props.clipped ? 'Release clipping mask' : 'Create clipping mask', keys: 'Ctrl+Alt+G', off: !any },
        { id: 'layer-style', label: 'Layer style…', off: !any || props.layerKind === 'adjustment' },
        { sep: true },
        { id: 'new-adjustment', label: 'New adjustment layer', items: props.adjustments.map((a) => ({ id: `adj-${a.type}`, label: a.label, off: !doc })), off: !doc },
        { sep: true },
        { id: 'mask-add', label: props.hasSelection ? 'Add mask from the selection' : 'Add mask (all shown)', off: !any || props.hasMask },
        { id: 'mask-invert', label: 'Invert mask', off: !props.hasMask },
        { id: 'mask-delete', label: 'Delete mask', off: !props.hasMask },
        { sep: true },
        { id: 'merge-down', label: 'Merge down', keys: 'Ctrl+E', off: !props.canMergeDown },
        { id: 'flatten', label: 'Flatten image', off: !doc },
      ],
    },
    {
      id: 'select',
      label: 'Select',
      items: [
        { id: 'select-all', label: 'All', keys: 'Ctrl+A', off: !doc },
        { id: 'select-none', label: 'Deselect', keys: 'Ctrl+D', off: !props.hasSelection },
        { id: 'select-invert', label: 'Inverse', keys: 'Ctrl+Shift+I', off: !doc },
        { sep: true },
        { id: 'select-subject', label: 'Subject', off: !doc },
        { id: 'select-sky', label: 'Sky', off: !doc },
        { id: 'select-range', label: 'Colour range…', off: !doc },
        { sep: true },
        { id: 'quick-mask', label: 'Quick mask', keys: 'Q', off: !doc },
        { id: 'select-mask', label: 'Select and Mask…', keys: 'Ctrl+Alt+R', off: !doc },
        { id: 'select-refine', label: 'Refine edge…', off: !props.hasSelection },
      ],
    },
    {
      id: 'filter',
      label: 'Filter',
      items: [
        { id: 'liquify', label: 'Liquify…', keys: 'Shift+Ctrl+X', off: !image },
        { sep: true },
        ...groups.map((g) => ({ ...g, off: !image })),
      ],
    },
    {
      id: 'view',
      label: 'View',
      items: [
        { id: 'zoom-in', label: 'Zoom in', keys: 'Ctrl++', off: !doc },
        { id: 'zoom-out', label: 'Zoom out', keys: 'Ctrl+-', off: !doc },
        { id: 'zoom-fit', label: 'Fit on screen', keys: 'Ctrl+0', off: !doc },
        { id: 'zoom-100', label: '100%', keys: 'Ctrl+1', off: !doc },
        { sep: true },
        { id: 'rulers', label: 'Rulers', keys: 'Ctrl+R' },
        { id: 'guides-show', label: 'Show guides', keys: 'Ctrl+;' },
        { id: 'guides-lock', label: 'Lock guides', keys: 'Alt+Ctrl+;' },
        { id: 'guide-new', label: 'New guide…', off: !doc },
        { id: 'guides-clear', label: 'Clear guides', off: !props.hasGuides },
        { sep: true },
        { id: 'grid', label: 'Grid', keys: "Ctrl+'" },
        { id: 'grid-size', label: 'Grid size…', off: !doc },
        { id: 'snap', label: 'Snap', keys: 'Shift+Ctrl+;' },
      ],
    },
    {
      id: 'window',
      label: 'Window',
      items: [...props.panels.map((p) => ({ id: `panel-${p.id}`, label: p.label })), { sep: true }, { id: 'workspace-reset', label: 'Reset the workspace' }],
    },
  ]
})

function toggle(id) {
  open.value = open.value === id ? '' : id
  sub.value = ''
}

function run(item) {
  if (item.off) return
  if (item.items) {
    sub.value = sub.value === item.id ? '' : item.id
    return
  }
  open.value = ''
  sub.value = ''
  emit('run', item.id)
}

function onWindow(event) {
  if (open.value && !bar.value?.contains(event.target)) {
    open.value = ''
    sub.value = ''
  }
}

function onKey(event) {
  if (open.value && event.key === 'Escape') {
    if (sub.value) sub.value = ''
    else open.value = ''
    event.stopPropagation()
  }
}

onMounted(() => {
  window.addEventListener('pointerdown', onWindow)
  window.addEventListener('keydown', onKey, true)
})
onBeforeUnmount(() => {
  window.removeEventListener('pointerdown', onWindow)
  window.removeEventListener('keydown', onKey, true)
})
</script>

<template>
  <nav ref="bar" class="menubar" aria-label="Menus">
    <div v-for="m in menus" :key="m.id" class="menu">
      <button type="button" class="top" :class="{ on: open === m.id }" :aria-expanded="open === m.id" aria-haspopup="menu" @click="toggle(m.id)" @pointerenter="open && open !== m.id && ((open = m.id), (sub = ''))">
        {{ m.label }}
      </button>
      <div v-if="open === m.id" class="drop" role="menu" :aria-label="m.label">
        <template v-for="(item, i) in m.items" :key="item.id || `sep-${i}`">
          <hr v-if="item.sep" />
          <div v-else-if="item.items" class="has-sub" @pointerenter="!item.off && (sub = item.id)">
            <button type="button" role="menuitem" aria-haspopup="menu" :aria-expanded="sub === item.id" :disabled="item.off" :class="{ on: sub === item.id }" @click="run(item)">
              <span>{{ item.label }}</span>
              <span class="more" aria-hidden="true">›</span>
            </button>
            <div v-if="sub === item.id" class="drop side" role="menu" :aria-label="item.label">
              <p v-if="!item.items.length" class="none">Nothing here yet</p>
              <button v-for="s in item.items" :key="s.id" type="button" role="menuitem" :disabled="s.off" @click="run(s)">
                <span>{{ s.label }}</span>
                <kbd v-if="s.keys">{{ s.keys }}</kbd>
              </button>
            </div>
          </div>
          <button v-else type="button" :role="item.id in checks ? 'menuitemcheckbox' : 'menuitem'" :aria-checked="item.id in checks ? !!checks[item.id] : undefined" :disabled="item.off" @click="run(item)" @pointerenter="sub = ''">
            <span><span class="tick" aria-hidden="true">{{ checks[item.id] ? '✓' : '' }}</span>{{ item.label }}</span>
            <kbd v-if="item.keys">{{ item.keys }}</kbd>
          </button>
        </template>
      </div>
    </div>
  </nav>
</template>

<style scoped>
.menubar {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 2px;
}

.menu {
  position: relative;
}

.top {
  padding: 4px 11px;
  border: 0;
  border-radius: 8px;
  background: none;
  color: var(--text);
  font-size: 14px;
}

.top:hover,
.top.on {
  background: color-mix(in srgb, var(--accent) 18%, transparent);
}

.drop {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  z-index: 40;
  display: flex;
  flex-direction: column;
  min-width: 260px;
  padding: 5px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: var(--panel-background);
  backdrop-filter: blur(var(--glass-blur));
  box-shadow: 0 12px 34px rgba(0, 0, 0, 0.5);
}

.has-sub {
  position: relative;
  display: flex;
  flex-direction: column;
}

.drop.side {
  top: -6px;
  left: calc(100% + 2px);
  min-width: 230px;
  max-height: min(70vh, 640px);
  overflow-y: auto;
}

.drop button {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 18px;
  padding: 6px 10px;
  border: 0;
  border-radius: 7px;
  background: none;
  color: var(--text);
  font-size: 13px;
  text-align: left;
}

.drop button:hover:not(:disabled),
.drop button.on {
  background: color-mix(in srgb, var(--accent) 22%, transparent);
}

.tick {
  display: inline-block;
  width: 16px;
  color: var(--accent);
}

.more {
  color: var(--text-dim);
  font-size: 15px;
}

.none {
  margin: 0;
  padding: 6px 10px;
  color: var(--text-dim);
  font-size: 12.5px;
}

kbd {
  color: var(--text-dim);
  font: inherit;
  font-size: 11.5px;
}

hr {
  width: 100%;
  margin: 4px 0;
  border: 0;
  border-top: 1px solid var(--line);
}

button {
  font: inherit;
  cursor: pointer;
}

button:disabled {
  opacity: 0.4;
  cursor: default;
}

button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 1px;
}
</style>
