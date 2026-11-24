<script setup>
// FINANCE: the receipts the companion reads and proposes, kept only when the member presses Keep.
// Everything here lives in the member's own memory database and never leaves this computer.
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { chatMediaOpen, financeDecide, financeDrop, financeExport, financeMonth } from '../api/castle.js'
import { CATEGORIES, byCurrency, categoryLabel, draftOf, dropLine, edited, money, monthTitle, shiftMonth } from '../finance.js'
import GlassSelect from '../components/GlassSelect.vue'
import '../components/automations/roomcard.css'

const props = defineProps({ companion: { type: String, default: 'Your companion' } })

const POLL_MS = 5000
const data = ref(null)
const month = ref(null)
const problem = ref('')
const notice = ref('')
const busy = ref(false)
const dropping = ref(false)
// the edit form of each waiting receipt, by id
const drafts = ref({})

const groups = computed(() => byCurrency(data.value?.totals ?? []))
const waiting = computed(() => data.value?.waiting ?? [])
const kept = computed(() => data.value?.kept ?? [])
const picture = (r) => (r.pictureFile ? convertFileSrc(r.pictureFile) : '')

async function load() {
  try {
    const next = await financeMonth(month.value)
    data.value = next
    month.value = next.month
    for (const r of next.waiting) if (!drafts.value[r.id]) drafts.value[r.id] = draftOf(r)
    problem.value = ''
  } catch (e) {
    problem.value = e?.message || String(e)
  }
}

function go(delta) {
  month.value = shiftMonth(month.value, delta)
  load()
}

async function decide(r, keep) {
  busy.value = true
  notice.value = ''
  try {
    const draft = drafts.value[r.id]
    await financeDecide(r.id, keep, keep && draft && edited(r, draft) ? draft : null)
    delete drafts.value[r.id]
    await load()
  } catch (e) {
    problem.value = e?.message || String(e)
  } finally {
    busy.value = false
  }
}

async function exportCsv(all) {
  busy.value = true
  try {
    const r = await financeExport(all ? null : month.value)
    if (r) notice.value = `Saved ${r.count} receipt${r.count === 1 ? '' : 's'} to ${r.path}.`
  } catch (e) {
    problem.value = e?.message || String(e)
  } finally {
    busy.value = false
  }
}

async function onDragDrop(event) {
  const drop = event.payload
  if (drop.type === 'enter' || drop.type === 'over') dropping.value = true
  else if (drop.type === 'leave') dropping.value = false
  else if (drop.type === 'drop') {
    dropping.value = false
    if (!drop.paths?.length) return
    busy.value = true
    try {
      notice.value = dropLine(await financeDrop(drop.paths), props.companion)
      problem.value = ''
    } catch (e) {
      problem.value = e?.message || String(e)
    } finally {
      busy.value = false
    }
  }
}

let timer
let stopDrop
onMounted(async () => {
  await load()
  timer = setInterval(load, POLL_MS)
  stopDrop = await getCurrentWebview().onDragDropEvent(onDragDrop)
})
onBeforeUnmount(() => {
  clearInterval(timer)
  stopDrop?.()
})
</script>

<template>
  <section class="finance" :class="{ dropping }" aria-label="Finance">
    <header class="head">
      <div>
        <h1>FINANCE</h1>
        <p class="quiet">Drop receipt pictures here, or send them to {{ companion }} from your phone. They stay on this computer.</p>
      </div>
      <div class="month" v-if="month">
        <button title="The month before" aria-label="The month before" @click="go(-1)">‹</button>
        <strong>{{ monthTitle(month) }}</strong>
        <button title="The month after" aria-label="The month after" @click="go(1)">›</button>
      </div>
    </header>

    <p v-if="problem" class="problem" role="alert">{{ problem }}</p>
    <p v-if="notice" class="notice" role="status">{{ notice }}</p>

    <section v-if="waiting.length" class="castle-room-card live waiting">
      <h2>Waiting for your yes · {{ waiting.length }}</h2>
      <article v-for="r in waiting" :key="r.id" class="receipt">
        <button class="thumb" :title="'Open the picture'" @click="chatMediaOpen(r.pictureFile)">
          <img v-if="picture(r)" :src="picture(r)" alt="The receipt" />
        </button>
        <div v-if="drafts[r.id]" class="fields">
          <label>Shop <input v-model="drafts[r.id].shop" maxlength="120" /></label>
          <label>Total <input v-model="drafts[r.id].total" inputmode="decimal" /></label>
          <label>Currency <input v-model="drafts[r.id].currency" maxlength="3" class="short" /></label>
          <label>Date <input v-model="drafts[r.id].date" type="date" /></label>
          <label>Category
            <GlassSelect v-model="drafts[r.id].category" :options="CATEGORIES.map((c) => ({ value: c, label: categoryLabel(c) }))" aria-label="Category" />
          </label>
          <label>Card (last 4) <input v-model="drafts[r.id].last4" maxlength="4" class="short" inputmode="numeric" /></label>
          <label class="wide">Note <input v-model="drafts[r.id].note" maxlength="255" /></label>
        </div>
        <div class="actions">
          <button class="keep" :disabled="busy" @click="decide(r, true)">Keep</button>
          <button :disabled="busy" @click="decide(r, false)">Dismiss</button>
        </div>
      </article>
    </section>

    <div class="columns">
      <section class="castle-room-card">
        <h2>This month</h2>
        <p v-if="!groups.length" class="quiet">Nothing kept this month yet.</p>
        <div v-for="g in groups" :key="g.currency" class="currency">
          <p class="total">{{ money(g.cents, g.currency) }}</p>
          <div v-for="row in g.rows" :key="row.category" class="bar-row">
            <span class="label">{{ categoryLabel(row.category) }}</span>
            <span class="bar"><span :style="{ width: Math.max(2, row.share * 100) + '%' }"></span></span>
            <span class="amount">{{ money(row.cents, g.currency) }}</span>
          </div>
        </div>
      </section>

      <section class="castle-room-card">
        <div class="latest-head">
          <h2>Latest</h2>
          <span class="spacer"></span>
          <button :disabled="busy" @click="exportCsv(false)">Export month</button>
          <button :disabled="busy" @click="exportCsv(true)">Export all</button>
        </div>
        <p v-if="!kept.length" class="quiet">No receipts kept this month.</p>
        <button v-for="r in kept" :key="r.id" class="line" :title="'Open the picture'" @click="chatMediaOpen(r.pictureFile)">
          <span class="date">{{ r.spentOn }}</span>
          <span class="shop">{{ r.shop }}</span>
          <span class="cat">{{ categoryLabel(r.category) }}</span>
          <span class="amount">{{ money(r.totalCents, r.currency) }}</span>
        </button>
      </section>
    </div>

    <div v-if="dropping" class="drop-veil">Drop the receipts: {{ companion }} reads each one.</div>
  </section>
</template>

<style scoped>
.finance {
  position: relative;
  display: flex;
  flex-direction: column;
  gap: 16px;
  height: 100%;
  padding: 24px;
  overflow-y: auto;
}

.head {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 16px;
}

h1 {
  margin: 0;
  letter-spacing: 0.12em;
}

h2 {
  margin: 0 0 12px;
  font-size: 14px;
  letter-spacing: 0.06em;
}

.quiet {
  margin: 4px 0 0;
  color: var(--text-dim);
}

.month {
  display: flex;
  align-items: center;
  gap: 10px;
}

.problem {
  color: var(--danger, #e07070);
}

.notice {
  color: var(--text-dim);
}

.receipt {
  display: grid;
  grid-template-columns: 96px 1fr auto;
  gap: 14px;
  align-items: start;
  padding: 12px 0;
  border-top: 1px solid var(--line);
}

.thumb {
  width: 96px;
  height: 128px;
  padding: 0;
  overflow: hidden;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: none;
  cursor: zoom-in;
}

.thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.fields {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  gap: 8px 12px;
}

.fields label {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 12px;
  color: var(--text-dim);
}

.fields .wide {
  grid-column: 1 / -1;
}

.fields .short {
  max-width: 80px;
}

.actions {
  display: flex;
  flex-direction: column;
  gap: 8px;
}

.keep {
  border-color: var(--accent);
}

.columns {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(320px, 1fr));
  gap: 16px;
}

.currency + .currency {
  margin-top: 16px;
}

.total {
  margin: 0 0 8px;
  font-size: 20px;
}

.bar-row {
  display: grid;
  grid-template-columns: 90px 1fr auto;
  gap: 10px;
  align-items: center;
  margin: 6px 0;
  font-size: 13px;
}

.bar {
  height: 8px;
  border-radius: 4px;
  background: color-mix(in srgb, var(--accent) 12%, transparent);
}

.bar span {
  display: block;
  height: 100%;
  border-radius: 4px;
  background: var(--accent);
}

.latest-head {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-bottom: 8px;
}

.latest-head h2 {
  margin: 0;
}

.spacer {
  flex: 1;
}

.line {
  display: grid;
  grid-template-columns: 90px 1fr auto auto;
  gap: 10px;
  width: 100%;
  padding: 6px 0;
  border: none;
  border-top: 1px solid var(--line);
  background: none;
  color: inherit;
  text-align: left;
  cursor: pointer;
}

.date,
.cat {
  color: var(--text-dim);
}

.drop-veil {
  position: absolute;
  inset: 12px;
  display: grid;
  place-items: center;
  border: 2px dashed var(--accent);
  border-radius: 18px;
  background: color-mix(in srgb, var(--panel-background, #000) 80%, transparent);
  font-size: 18px;
  pointer-events: none;
}
</style>
