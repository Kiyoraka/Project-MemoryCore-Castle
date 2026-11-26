<script setup>
// One card component for HOME's memory widgets, so every card shares one look: TODAY,
// PROJECTS, DIARY, GROWTH, BACKUP and DATABASE. Each shows a friendly empty line when the member's
// memory has nothing there yet (a fresh MemoryCore), never an error.
import { computed, reactive, ref } from 'vue'
import { homeReminderDone } from '../../api/castle.js'
import { card as databaseCard } from '../database.js'
import { cardCount, cardItems, ticker } from './homecards.js'

const props = defineProps({
  kind: { type: String, required: true }, // today | projects | diary | growth | backup | database
  data: { type: Object, default: null }, // HomeData from memory_home
  backup: { type: Object, default: null }, // BackupStatus
  database: { type: Object, default: null }, // { engine, memory } from db_status and db_memory
})
const emit = defineEmits(['open', 'backup', 'database', 'changed'])
// TODAY: ○ ticks a reminder (database memories only); it stays struck through with Undo for a moment
const ticks = ticker((id, done) => homeReminderDone(id, done), () => emit('changed'), 6000, reactive(new Map()))
const tickError = ref('')
async function tick(id) {
  tickError.value = ''
  try { await ticks.tick(id) } catch (e) { tickError.value = String(e?.message ?? e) }
}
async function untick(id) {
  tickError.value = ''
  try { await ticks.undo(id) } catch (e) { tickError.value = String(e?.message ?? e) }
}
const db = computed(() => databaseCard(props.database?.engine ?? null, props.database?.memory ?? null))

const TITLES = { today: 'TODAY', projects: 'PROJECTS', diary: 'DIARY', growth: 'GROWTH', backup: 'BACKUP', database: 'DATABASE' }
const ICONS = { today: '▦', projects: '▤', diary: '❒', growth: '↗', backup: '⇪', database: '⛁' }

function ago(iso) {
  if (!iso) return 'never'
  const ms = Date.now() - new Date(iso).getTime()
  if (Number.isNaN(ms)) return iso
  const minutes = Math.round(ms / 60000)
  if (minutes < 60) return `${Math.max(1, minutes)} min ago`
  const hours = Math.round(minutes / 60)
  if (hours < 48) return `${hours} h ago`
  return `${Math.round(hours / 24)} days ago`
}

function due(date) {
  if (!date) return ''
  const when = new Date(`${date}T00:00:00`)
  return Number.isNaN(when.getTime()) ? date : when.toLocaleDateString('en-US', { month: 'short', day: 'numeric' })
}
</script>

<template>
  <section class="widget card" :class="kind">
    <h3><span class="icon">{{ ICONS[kind] }}</span>{{ TITLES[kind] }}<span v-if="kind === 'today' || kind === 'projects'" class="count">{{ cardCount(kind, kind === 'today' ? data?.reminders : data?.projects) }}</span></h3>

    <template v-if="kind === 'today'">
      <ul v-if="data?.reminders?.length" class="scroll">
        <li v-for="r in cardItems(data.reminders)" :key="r.id ?? r.title" :title="r.detail || r.title" :class="{ ticked: ticks.has(r.id) }">
          <button v-if="r.id != null" type="button" class="dot tick" :aria-label="ticks.has(r.id) ? `Done: ${r.title}` : `Mark done: ${r.title}`" :disabled="ticks.has(r.id)" @click="tick(r.id)">{{ ticks.has(r.id) ? '✓' : '○' }}</button>
          <span v-else class="dot">○</span>
          <span class="text">{{ r.title }}</span>
          <button v-if="ticks.has(r.id)" type="button" class="undo" @click="untick(r.id)">Undo</button>
          <span v-else class="side">{{ due(r.due) }}</span>
        </li>
      </ul>
      <p v-if="tickError" class="tick-error">{{ tickError }}</p>
      <p v-else class="empty">{{ data?.features?.reminders ? 'Nothing open - a calm day.' : 'Ask your companion to remind you of something.' }}</p>
    </template>

    <template v-else-if="kind === 'projects'">
      <ul v-if="data?.projects?.length" class="scroll">
        <li v-for="(p, i) in cardItems(data.projects)" :key="p.name" class="link" @click="$emit('open', p.file)">
          <span class="num">{{ i + 1 }}</span><span class="text">{{ p.name }}</span>
          <span v-if="p.tree?.kids || p.tree?.tasks" class="side progress-side" :title="`${p.tree.kids ? `${p.tree.kids} sub-project${p.tree.kids === 1 ? '' : 's'}` : ''}${p.tree.kids && p.tree.tasks ? ' · ' : ''}${p.tree.tasks ? `${p.tree.done} of ${p.tree.tasks} tasks done` : ''}`">{{ p.tree.tasks ? `${p.tree.done}/${p.tree.tasks}` : `${p.tree.kids} sub` }} ›</span>
          <span v-else class="side">›</span>
        </li>
      </ul>
      <p v-else class="empty">No projects yet.</p>
    </template>

    <template v-else-if="kind === 'diary'">
      <p v-if="data?.diary" class="link diary-line" @click="$emit('open', data.diary.file)">
        <span class="text">{{ data.growth?.days ? `Day ${data.growth.days} - ` : '' }}{{ data.diary.title }}</span><span class="side">›</span>
      </p>
      <p v-else class="empty">Press Save diary after a talk to write the first page.</p>
    </template>

    <template v-else-if="kind === 'growth'">
      <p class="growth-line">
        <span>{{ data?.growth?.memories ?? 0 }} memories</span>
        <span class="sep">·</span>
        <span>{{ data?.growth?.days ?? 0 }} days</span>
        <span class="sep">·</span>
        <span>{{ data?.growth?.skills ?? 0 }} skills</span>
      </p>
    </template>

    <template v-else-if="kind === 'backup'">
      <p v-if="!backup" class="empty">Checking…</p>
      <template v-else-if="backup.ready">
        <p class="facts">{{ ago(backup.lastBackup) }} · {{ backup.changed }} changed</p>
        <button @click="$emit('backup')">Back up now</button>
      </template>
      <template v-else>
        <p class="empty">Not set up yet.</p>
        <button @click="$emit('backup', 'setup')">Set up backup</button>
      </template>
    </template>

    <template v-else-if="kind === 'database'">
      <p :class="db.button ? 'empty' : 'facts'">{{ db.line }}</p>
      <button v-if="db.button" @click="$emit('database', db.button.action)">{{ db.button.label }}</button>
    </template>
  </section>
</template>

<style scoped>
.card {
  padding: 14px 18px;
  box-sizing: border-box;
}
h3 {
  margin: 0 0 10px;
  padding-bottom: 8px;
  border-bottom: 1px solid var(--line);
  font-size: 13px;
  font-weight: 500;
  letter-spacing: 0.14em;
}
.icon {
  margin-right: 10px;
  color: var(--accent);
}
ul {
  display: grid;
  gap: 8px;
  margin: 0;
  padding: 0;
  list-style: none;
}
/* TODAY and PROJECTS are never taller than four one-line rows (the card's height before it
   listed everything) and scroll for the rest. HOME places widgets freely with no collision check,
   so a taller card would cover the one below it (W4). */
ul.scroll {
  max-height: calc(4 * 22px + 3 * 8px);
  overflow-y: auto;
  overscroll-behavior: contain;
  padding-right: 4px;
}
.count {
  float: right;
  font-size: 11px;
  letter-spacing: 0.06em;
  color: var(--text-dim);
}
li,
.diary-line {
  display: flex;
  gap: 10px;
  align-items: center;
  margin: 0;
  font-size: 14px;
}
.text {
  flex: 1;
  min-width: 0; /* a flex item will not shrink below its text without this, so the line ran off the card */
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
}
/* a reminder is read, not skimmed: up to two lines, then … (the full text is its tooltip) */
.today .text {
  display: -webkit-box;
  -webkit-box-orient: vertical;
  -webkit-line-clamp: 2;
  white-space: normal;
  overflow-wrap: anywhere;
  line-height: 1.35;
}
.today li {
  align-items: flex-start;
}
.side {
  font-size: 12px;
  color: var(--text-dim);
}
.dot {
  color: var(--text-dim);
}
button.tick {
  padding: 0;
  border: 0;
  background: none;
  font: inherit;
  line-height: inherit;
  cursor: pointer;
}
button.tick:hover:not(:disabled),
button.tick:focus-visible {
  color: var(--accent);
}
button.tick:disabled {
  color: var(--accent);
  cursor: default;
}
.ticked .text {
  text-decoration: line-through;
  color: var(--text-dim);
}
.undo {
  padding: 0 4px;
  border: 0;
  background: none;
  font-size: 12px;
  color: var(--accent);
  cursor: pointer;
}
.tick-error {
  margin: 8px 0 0;
  font-size: 12px;
  color: var(--danger, #e66);
}
.num {
  display: grid;
  place-items: center;
  width: 22px;
  height: 22px;
  border: 1px solid var(--line);
  border-radius: 50%;
  font-size: 12px;
}
.link {
  cursor: pointer;
}
.link:hover .text {
  color: var(--accent);
}
.empty,
.facts {
  margin: 0 0 6px;
  font-size: 13px;
  color: var(--text-dim);
}
.growth-line {
  display: flex;
  gap: 10px;
  margin: 0;
  font-size: 15px;
}
.sep {
  color: var(--text-dim);
}
button {
  padding: 5px 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--hover);
  color: var(--text);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}
</style>
