<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref } from 'vue'
import GlassSelect from '../../components/GlassSelect.vue'
import { BLEND_MODES_3, blendLabel, hasStyles, isGroup, layerRows, styleList } from './viewHelpers.js'

// PHOTO's LAYERS v3 (photo-v3-1): the chosen layer's blend (all 27 modes) and opacity on top; rows top
// first - groups with a fold arrow and their members indented, a clipped layer with its link arrow, an fx
// badge that opens the layer's effects (each with its own eye). A row drags above, below or into a group;
// Alt+click clips a layer to the one under it (again releases it). The mask thumbnail works as in v2
// (click: paint it, Alt+click: see it alone, right-click: more). The footer: clip, fx, mask, group, new, delete.
const props = defineProps({
  layers: { type: Array, default: () => [] },
  selectedId: { type: String, default: '' },
  editMask: { type: Boolean, default: false },
  viewMask: { type: String, default: '' },
  blends: { type: Array, default: () => BLEND_MODES_3 },
})
const emit = defineEmits([
  'select', 'edit-mask', 'view-mask', 'toggle', 'rename', 'set', 'place', 'duplicate', 'remove', 'add',
  'add-mask', 'delete-mask', 'invert-mask', 'group', 'ungroup', 'fold', 'clip', 'style-toggle', 'open-style',
])

const ADJ_ICONS = {
  curves: 'M4 4h16v16H4ZM6 18c4 0 4-12 12-12',
  levels: 'M4 20V10m4 10V6m4 14v-8m4 8V4m4 16v-6',
  hsl: 'M12 3s6 6.4 6 11a6 6 0 0 1-12 0c0-4.6 6-11 6-11Z',
  adjust: 'M4 6h10m4 0h2M4 12h4m4 0h8M4 18h12M16 4v4M10 10v4M18 16v4',
}
const HALF = 'M12 4a8 8 0 1 0 0 16 8 8 0 0 0 0-16Zm0 0v16'

const rows = computed(() => layerRows(props.layers))
const selected = computed(() => props.layers.find((l) => l.id === props.selectedId) || null)
const renaming = ref('')
const draft = ref('')
const nameBox = ref(null)
const panel = ref(null)
const menu = ref(null) // { id, x, y, kind: 'mask' | 'layer' }
const dragId = ref('')
const over = ref(null) // { id, mode }
const openFx = ref(new Set())

function pickLayer(l, event) {
  if (event?.altKey) {
    emit('clip', l.id)
    return
  }
  emit('select', l.id)
  emit('edit-mask', false)
}

function pickMask(l, event) {
  emit('select', l.id)
  if (event.altKey) {
    emit('view-mask', props.viewMask === l.id ? '' : l.id)
    return
  }
  emit('edit-mask', true)
}

function openMenu(l, event, kind) {
  event.preventDefault()
  emit('select', l.id)
  // the panel's glass makes it the frame for anything fixed inside it: place it inside the panel
  const r = panel.value.getBoundingClientRect()
  menu.value = { id: l.id, kind, x: Math.max(4, Math.min(event.clientX - r.left, r.width - 190)), y: Math.min(event.clientY - r.top, r.height - 40) }
}

function closeMenu() {
  menu.value = null
}

const menuLayer = computed(() => props.layers.find((l) => l.id === menu.value?.id) || null)

async function startRename(layer) {
  renaming.value = layer.id
  draft.value = layer.name
  await nextTick()
  nameBox.value?.[0]?.focus()
  nameBox.value?.[0]?.select()
}

function finishRename(save) {
  const id = renaming.value
  if (!id) return
  renaming.value = ''
  if (save && draft.value.trim()) emit('rename', id, draft.value.trim())
}

function toggleFx(id) {
  const next = new Set(openFx.value)
  if (next.has(id)) next.delete(id)
  else next.add(id)
  openFx.value = next
}

// Where a drop lands: the top third above, the bottom third below, the middle into a group.
function dropMode(l, event) {
  const r = event.currentTarget.getBoundingClientRect()
  const t = (event.clientY - r.top) / Math.max(1, r.height)
  if (isGroup(l) && t > 0.28 && t < 0.72) return 'into'
  return t < 0.5 ? 'above' : 'below'
}

function onDragStart(l, event) {
  dragId.value = l.id
  event.dataTransfer.effectAllowed = 'move'
  event.dataTransfer.setData('text/plain', l.id)
}

function onDragOver(l, event) {
  if (!dragId.value || dragId.value === l.id) return
  over.value = { id: l.id, mode: dropMode(l, event) }
}

function onDrop(l, event) {
  const id = dragId.value
  const mode = dropMode(l, event)
  dragId.value = ''
  over.value = null
  if (id && id !== l.id) emit('place', id, l.id, mode)
}

function endDrag() {
  dragId.value = ''
  over.value = null
}

function onWindow(event) {
  if (menu.value && !event.target.closest?.('.ctx')) menu.value = null
}

onMounted(() => window.addEventListener('pointerdown', onWindow))
onBeforeUnmount(() => window.removeEventListener('pointerdown', onWindow))
</script>

<template>
  <section ref="panel" class="layers" aria-label="Layers">
    <div class="top">
      <GlassSelect
        :model-value="selected?.blend || 'normal'"
        :disabled="!selected || selected.kind === 'adjustment'"
        aria-label="Blend mode"
        :options="(selected?.kind === 'group' ? ['passThrough', ...blends] : blends).map((id) => ({ value: id, label: blendLabel(id) }))"
        @change="(v) => emit('set', selected.id, { blend: v }, true)"
      />
      <label class="opacity">
        <span>Opacity</span>
        <input
          type="range"
          min="0"
          max="100"
          :disabled="!selected"
          :value="Math.round((selected?.opacity ?? 1) * 100)"
          aria-label="Layer opacity"
          @input="emit('set', selected.id, { opacity: $event.target.value / 100 }, false)"
          @change="emit('set', selected.id, { opacity: $event.target.value / 100 }, true)"
        />
        <span class="num">{{ Math.round((selected?.opacity ?? 1) * 100) }}%</span>
      </label>
    </div>
    <p v-if="!rows.length" class="dim">No layers yet.</p>
    <ul>
      <template v-for="r in rows" :key="r.layer.id">
        <li
          class="row"
          :class="{
            on: r.layer.id === selectedId,
            dragging: dragId === r.layer.id,
            'drop-above': over?.id === r.layer.id && over.mode === 'above',
            'drop-below': over?.id === r.layer.id && over.mode === 'below',
            'drop-into': over?.id === r.layer.id && over.mode === 'into',
          }"
          :style="{ paddingLeft: `${6 + r.depth * 18}px` }"
          draggable="true"
          :title="r.layer.clipped ? 'Clipped to the layer below (Alt+click to release)' : 'Alt+click to clip it to the layer below'"
          @dragstart="onDragStart(r.layer, $event)"
          @dragend="endDrag"
          @dragover.prevent="onDragOver(r.layer, $event)"
          @dragleave="over?.id === r.layer.id && (over = null)"
          @drop.prevent="onDrop(r.layer, $event)"
          @click="pickLayer(r.layer, $event)"
          @contextmenu="openMenu(r.layer, $event, 'layer')"
        >
          <button
            type="button"
            class="eye"
            :class="{ off: !r.layer.visible }"
            :aria-pressed="r.layer.visible"
            :aria-label="r.layer.visible ? 'Hide layer' : 'Show layer'"
            :title="r.layer.visible ? 'Hide' : 'Show'"
            @click.stop="emit('toggle', r.layer.id)"
          >
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="M2 12s3.6-6.5 10-6.5S22 12 22 12s-3.6 6.5-10 6.5S2 12 2 12Z" />
              <circle cx="12" cy="12" r="3" />
              <path v-if="!r.layer.visible" d="M4 4l16 16" />
            </svg>
          </button>
          <button
            v-if="r.layer.kind === 'group'"
            type="button"
            class="arrow"
            :aria-label="r.layer.collapsed ? 'Open the group' : 'Fold the group'"
            :aria-expanded="!r.layer.collapsed"
            @click.stop="emit('fold', r.layer.id, !r.layer.collapsed)"
          >
            <svg viewBox="0 0 24 24" aria-hidden="true"><path :d="r.layer.collapsed ? 'm9 6 6 6-6 6' : 'm6 9 6 6 6-6'" /></svg>
          </button>
          <span v-if="r.layer.clipped" class="clip" aria-label="Clipped" title="Clipped to the layer below">
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M8 4v9a4 4 0 0 0 4 4h6m-3-3 3 3-3 3" /></svg>
          </span>
          <button
            type="button"
            class="thumb"
            :class="{ chosen: r.layer.id === selectedId && !editMask, adj: r.layer.kind === 'adjustment' || r.layer.kind === 'group' || r.layer.kind === 'text' }"
            :title="r.layer.kind === 'adjustment' ? 'Adjustment layer' : r.layer.kind === 'group' ? 'Group' : 'Edit the layer'"
            @click.stop="pickLayer(r.layer, $event)"
          >
            <img v-if="r.layer.kind === 'image' && r.layer.png" :src="r.layer.png" alt="" draggable="false" />
            <span v-else-if="r.layer.kind === 'text'" class="glyph">T</span>
            <svg v-else-if="r.layer.kind === 'group'" viewBox="0 0 24 24" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z" /></svg>
            <svg v-else-if="r.layer.kind === 'adjustment'" viewBox="0 0 24 24" aria-hidden="true"><path :d="ADJ_ICONS[r.layer.adjustment?.type] || HALF" /></svg>
          </button>
          <button
            v-if="r.layer.mask"
            type="button"
            class="thumb mask"
            :class="{ chosen: r.layer.id === selectedId && editMask, viewing: viewMask === r.layer.id }"
            title="Mask - click to paint it (white shows, black hides), Alt+click to see it alone, right-click for more"
            @click.stop="pickMask(r.layer, $event)"
            @contextmenu.stop="openMenu(r.layer, $event, 'mask')"
          >
            <img :src="r.layer.mask" alt="" draggable="false" />
          </button>
          <input
            v-if="renaming === r.layer.id"
            ref="nameBox"
            v-model="draft"
            class="name-edit"
            maxlength="60"
            aria-label="Layer name"
            @click.stop
            @keydown.enter.prevent="finishRename(true)"
            @keydown.esc.prevent="finishRename(false)"
            @blur="finishRename(true)"
          />
          <span v-else class="name" :class="{ group: r.layer.kind === 'group' }" :title="`${r.layer.name} - double-click to rename`" @dblclick.stop="startRename(r.layer)">{{ r.layer.name }}</span>
          <button
            v-if="r.layer.styles && styleList(r.layer).length"
            type="button"
            class="fx"
            :class="{ off: !hasStyles(r.layer) }"
            :aria-expanded="openFx.has(r.layer.id)"
            :title="`Effects: ${styleList(r.layer).map((s) => s.name).join(', ')} - click to list them, double-click for Layer Style`"
            @click.stop="toggleFx(r.layer.id)"
            @dblclick.stop="emit('open-style', r.layer.id)"
          >
            <i>fx</i>
          </button>
        </li>
        <li v-for="s in openFx.has(r.layer.id) ? styleList(r.layer) : []" :key="`${r.layer.id}-${s.key}`" class="effect" :style="{ paddingLeft: `${40 + r.depth * 18}px` }" @dblclick="emit('open-style', r.layer.id)">
          <button type="button" class="eye small" :class="{ off: !s.on }" :aria-pressed="s.on" :aria-label="s.on ? `Hide ${s.name}` : `Show ${s.name}`" @click.stop="emit('style-toggle', r.layer.id, s.key)">
            <svg viewBox="0 0 24 24" aria-hidden="true">
              <path d="M2 12s3.6-6.5 10-6.5S22 12 22 12s-3.6 6.5-10 6.5S2 12 2 12Z" />
              <circle cx="12" cy="12" r="3" />
              <path v-if="!s.on" d="M4 4l16 16" />
            </svg>
          </button>
          <span>{{ s.name }}</span>
        </li>
      </template>
    </ul>
    <footer>
      <button type="button" title="Clip to the layer below / release (Ctrl+Alt+G)" aria-label="Clipping mask" :disabled="!selected" @click="emit('clip', selectedId)">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M9.5 14.5 14.5 9.5M8 11 5.5 13.5a3.5 3.5 0 0 0 5 5L13 16m-2-8 2.5-2.5a3.5 3.5 0 0 1 5 5L16 13" /></svg>
      </button>
      <button type="button" title="Layer style…" aria-label="Layer style" :disabled="!selected || selected.kind === 'adjustment'" @click="emit('open-style', selectedId)">
        <i class="fx-word">fx</i>
      </button>
      <button type="button" title="Add a mask (from the selection, or all white)" aria-label="Add mask" :disabled="!selected || !!selected.mask" @click="emit('add-mask', selected.id)">
        <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="4" y="4" width="16" height="16" rx="2" /><circle cx="12" cy="12" r="4" /></svg>
      </button>
      <button type="button" title="Group (Ctrl+G)" aria-label="Group" :disabled="!layers.length" @click="emit('group', selectedId)">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7Z" /></svg>
      </button>
      <button type="button" title="New empty layer (Ctrl+Shift+N)" aria-label="New layer" @click="emit('add')">
        <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="4" y="4" width="16" height="16" rx="2" /><path d="M12 8v8M8 12h8" /></svg>
      </button>
      <button type="button" class="bad" title="Delete layer" aria-label="Delete layer" :disabled="!selected || layers.length <= 1" @click="emit('remove', selectedId)">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 7h16M10 11v6m4-6v6M6 7l1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12M9 7V4h6v3" /></svg>
      </button>
    </footer>
    <div v-if="menu && menu.kind === 'mask'" class="ctx" role="menu" :style="{ left: `${menu.x}px`, top: `${menu.y}px` }">
      <button type="button" role="menuitem" @click="emit('invert-mask', menu.id), closeMenu()">Invert mask</button>
      <button type="button" role="menuitem" @click="emit('view-mask', viewMask === menu.id ? '' : menu.id), closeMenu()">{{ viewMask === menu.id ? 'Show the picture' : 'See the mask alone' }}</button>
      <button type="button" role="menuitem" class="bad" @click="emit('delete-mask', menu.id), closeMenu()">Delete mask</button>
    </div>
    <div v-else-if="menu && menuLayer" class="ctx" role="menu" :style="{ left: `${menu.x}px`, top: `${menu.y}px` }">
      <button type="button" role="menuitem" @click="emit('duplicate', menu.id), closeMenu()">Duplicate <kbd>Ctrl+J</kbd></button>
      <button type="button" role="menuitem" @click="startRename(menuLayer), closeMenu()">Rename</button>
      <button type="button" role="menuitem" @click="emit('group', menu.id), closeMenu()">Group <kbd>Ctrl+G</kbd></button>
      <button v-if="menuLayer.kind === 'group'" type="button" role="menuitem" @click="emit('ungroup', menu.id), closeMenu()">Ungroup <kbd>Ctrl+Shift+G</kbd></button>
      <button type="button" role="menuitem" @click="emit('clip', menu.id), closeMenu()">{{ menuLayer.clipped ? 'Release clipping mask' : 'Create clipping mask' }}</button>
      <button v-if="menuLayer.kind !== 'adjustment'" type="button" role="menuitem" @click="emit('open-style', menu.id), closeMenu()">Layer style…</button>
      <button type="button" role="menuitem" class="bad" :disabled="layers.length <= 1" @click="emit('remove', menu.id), closeMenu()">Delete</button>
    </div>
  </section>
</template>

<style scoped>
.layers {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 8px;
  min-height: 0;
  padding: 10px 12px 8px;
  border: 1px solid var(--line, rgba(190, 170, 255, 0.3));
  border-radius: 16px;
  background: var(--panel-background, rgba(24, 19, 66, 0.72));
  backdrop-filter: blur(var(--glass-blur, 8px));
  color: var(--text);
}

.top {
  display: flex;
  align-items: center;
  gap: 8px;
}

.top select {
  flex: 1 1 120px;
  min-width: 0;
  padding: 4px 6px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
}

.opacity {
  display: flex;
  flex: 1 1 130px;
  align-items: center;
  gap: 6px;
  min-width: 0;
  color: var(--text-dim);
  font-size: 12px;
}

.opacity input {
  flex: 1;
  min-width: 30px;
  accent-color: var(--accent);
}

.num {
  width: 34px;
  text-align: right;
  font-variant-numeric: tabular-nums;
}

ul {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 3px;
  min-height: 0;
  margin: 0;
  padding: 0;
  overflow-y: auto;
  list-style: none;
}

.row {
  display: flex;
  flex: none;
  align-items: center;
  gap: 6px;
  padding: 4px 6px;
  border: 1px solid transparent;
  border-radius: 10px;
  background: rgba(255, 255, 255, 0.03);
  cursor: pointer;
}

.row.on {
  border-color: color-mix(in srgb, var(--accent, #8a5cf0) 40%, transparent);
  background: color-mix(in srgb, var(--accent, #8a5cf0) 18%, transparent);
}

.row.dragging {
  opacity: 0.5;
}

.row.drop-above {
  box-shadow: inset 0 2px 0 var(--accent);
}

.row.drop-below {
  box-shadow: inset 0 -2px 0 var(--accent);
}

.row.drop-into {
  outline: 2px dashed var(--accent);
  outline-offset: -2px;
}

.effect {
  display: flex;
  flex: none;
  align-items: center;
  gap: 6px;
  padding: 1px 6px;
  color: var(--text-dim);
  font-size: 12px;
}

.eye,
.arrow,
footer button {
  display: grid;
  flex: none;
  place-items: center;
  padding: 0;
  border: 0;
  background: none;
  color: var(--text-dim);
}

.eye {
  width: 24px;
  height: 24px;
  color: var(--accent);
}

.eye.small {
  width: 20px;
  height: 20px;
}

.eye.small svg {
  width: 15px;
  height: 15px;
}

.eye.off {
  color: var(--text-dim);
  opacity: 0.6;
}

.arrow {
  width: 16px;
  height: 24px;
  margin-right: -4px;
}

.arrow svg {
  width: 14px;
  height: 14px;
}

.clip {
  display: grid;
  flex: none;
  place-items: center;
  color: var(--accent);
}

.clip svg {
  width: 15px;
  height: 15px;
}

svg {
  width: 18px;
  height: 18px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.6;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.thumb {
  display: grid;
  flex: none;
  place-items: center;
  width: 40px;
  height: 32px;
  padding: 0;
  overflow: hidden;
  border: 2px solid transparent;
  border-radius: 6px;
  background: repeating-conic-gradient(#d9d6e4 0 25%, #ffffff 0 50%) 0 0 / 10px 10px;
}

.thumb.adj {
  background: rgba(0, 0, 0, 0.3);
  color: var(--accent);
}

.thumb.mask {
  width: 32px;
  background: #000;
}

.thumb.chosen {
  border-color: var(--accent);
}

.thumb.viewing {
  box-shadow: 0 0 0 2px #fff;
}

.thumb img {
  width: 100%;
  height: 100%;
  object-fit: contain;
}

.glyph {
  color: var(--text);
  font-family: Georgia, 'Times New Roman', serif;
  font-size: 19px;
}

.name {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  font-size: 13.5px;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.name-edit {
  flex: 1;
  min-width: 0;
  padding: 3px 6px;
  border: 1px solid var(--accent);
  border-radius: 6px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 13.5px;
}

.fx {
  flex: none;
  padding: 0 4px;
  border: 0;
  background: none;
  color: var(--text);
  font-family: Georgia, serif;
  font-size: 14px;
}

.fx.off {
  color: var(--text-dim);
}

footer {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 4px;
  padding-top: 4px;
  border-top: 1px solid var(--line);
}

footer button {
  width: 30px;
  height: 28px;
  border-radius: 8px;
}

.fx-word {
  font-family: Georgia, serif;
  font-size: 15px;
}

footer button:hover:not(:disabled) {
  background: rgba(255, 255, 255, 0.06);
  color: var(--text);
}

footer button.bad:hover:not(:disabled) {
  color: var(--bad, #ff6b8b);
}

.ctx {
  position: absolute;
  z-index: 30;
  display: flex;
  flex-direction: column;
  min-width: 190px;
  padding: 4px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--panel-background);
  backdrop-filter: blur(var(--glass-blur));
  box-shadow: 0 10px 30px rgba(0, 0, 0, 0.45);
}

.ctx button {
  display: flex;
  justify-content: space-between;
  gap: 12px;
  padding: 6px 10px;
  border: 0;
  border-radius: 6px;
  background: none;
  color: var(--text);
  font-size: 12.5px;
  text-align: left;
}

.ctx kbd {
  color: var(--text-dim);
  font: inherit;
  font-size: 11px;
}

.ctx button:hover:not(:disabled) {
  background: color-mix(in srgb, var(--accent) 20%, transparent);
}

.ctx button.bad {
  color: var(--bad, #ff6b8b);
}

.dim {
  margin: 0;
  color: var(--text-dim);
  font-size: 12px;
}

button {
  font: inherit;
  cursor: pointer;
}

button:disabled,
select:disabled,
input:disabled {
  opacity: 0.4;
  cursor: default;
}

button:focus-visible,
select:focus-visible,
input:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
</style>
