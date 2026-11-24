<script setup>
// The Sentinel opened from its shield: the companion's sentence and Patrol now, what is open with
// its buttons (Fix, This is normal, Undo - each one the member's own click), what was settled,
// kept as normal or fixed, and the officer's log.
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { onSentinelChanged, sentinelAlarmsSeen, sentinelClick, sentinelFindings, sentinelLog, sentinelPatrolNow, sentinelStatus } from '../api/castle.js'
import { findingButtons, moveLabel, networkLine, patrolAgo, shieldLook } from './sentinel.js'

const emit = defineEmits(['settings'])
const status = ref(null)
const open = ref([])
const closed = ref([])
const log = ref('')
const busy = ref(0)
const said = ref('')
const problem = ref('')
const notesOpen = ref(false)
const closedOpen = ref(false)
const logOpen = ref(false)
let stop

const shown = computed(() => shieldLook(status.value))
const ready = computed(() => status.value?.supported && status.value?.on && status.value?.installed)
const alarms = computed(() => open.value.filter((f) => f.hint === 'alarm'))
const looks = computed(() => open.value.filter((f) => f.hint === 'look'))
const notes = computed(() => open.value.filter((f) => f.hint === 'note'))
const lastPatrol = computed(() => patrolAgo(status.value))

async function read() {
  try {
    status.value = await sentinelStatus()
    if (!status.value.problem) {
      open.value = await sentinelFindings(['open'])
      closed.value = await sentinelFindings(['settled', 'accepted', 'fixed'])
    }
    if (logOpen.value) log.value = await sentinelLog()
  } catch (reason) {
    problem.value = String(reason)
  }
}

async function patrolNow() {
  said.value = ''
  problem.value = ''
  busy.value = -1
  try {
    const done = await sentinelPatrolNow()
    said.value = done.line
  } catch (reason) {
    problem.value = String(reason)
  } finally {
    busy.value = 0
    read()
  }
}

async function press(finding, word) {
  said.value = ''
  problem.value = ''
  busy.value = finding.id
  try {
    said.value = await sentinelClick(finding.id, word)
  } catch (reason) {
    problem.value = String(reason)
  } finally {
    busy.value = 0
    read()
  }
}

async function toggleLog() {
  logOpen.value = !logOpen.value
  if (logOpen.value) log.value = await sentinelLog().catch(() => '')
}

const buttonText = (finding, word) => (word === 'fix' ? moveLabel(finding.fixMove) : word === 'accept' ? 'This is normal' : 'Undo')
const stateText = { settled: 'Settled', accepted: 'Normal', fixed: 'Fixed' }

onMounted(async () => {
  await read()
  sentinelAlarmsSeen().catch(() => {})
  stop = await onSentinelChanged(read)
})
onBeforeUnmount(() => stop?.())
</script>

<template>
  <div class="sentinel">
    <section class="head" :class="shown.look">
      <p class="line">{{ shown.line }}</p>
      <div class="row">
        <span class="when">{{ lastPatrol }}</span>
        <button v-if="ready" :disabled="busy !== 0 || status?.patrolling" @click="patrolNow">{{ busy === -1 || status?.patrolling ? 'On patrol…' : 'Patrol now' }}</button>
        <button v-else-if="status?.supported" @click="emit('settings')">Set it up</button>
      </div>
      <p v-if="status?.supported" class="network">{{ networkLine(status) }}</p>
      <p v-if="said" class="said">{{ said }}</p>
      <p v-if="problem" class="problem">{{ problem }}</p>
    </section>

    <template v-if="status?.supported">
      <section v-for="group in [{ name: 'Alarms', list: alarms }, { name: 'Worth a look', list: looks }]" :key="group.name" class="group">
        <template v-if="group.list.length">
          <h3>{{ group.name }}</h3>
          <article v-for="f in group.list" :key="f.id" class="finding" :class="f.hint">
            <div class="title">{{ f.title }}</div>
            <div v-if="f.detail" class="detail">{{ f.detail }}</div>
            <div v-if="f.stateWhy" class="why">{{ f.stateWhy }}</div>
            <div class="buttons">
              <button v-for="word in findingButtons(f)" :key="word" :class="{ quiet: word !== 'fix' }" :disabled="busy !== 0" @click="press(f, word)">{{ buttonText(f, word) }}</button>
              <span class="seen">since {{ f.firstSeen }}</span>
            </div>
          </article>
        </template>
      </section>
      <p v-if="ready && !alarms.length && !looks.length" class="calm">Nothing needs you right now.</p>

      <section v-if="notes.length" class="group">
        <button class="fold" @click="notesOpen = !notesOpen">{{ notesOpen ? '▾' : '▸' }} {{ notes.length }} small note{{ notes.length === 1 ? '' : 's' }}</button>
        <template v-if="notesOpen">
          <article v-for="f in notes" :key="f.id" class="finding note">
            <div class="title">{{ f.title }}</div>
            <div v-if="f.detail" class="detail">{{ f.detail }}</div>
            <div class="buttons">
              <button v-for="word in findingButtons(f)" :key="word" class="quiet" :disabled="busy !== 0" @click="press(f, word)">{{ buttonText(f, word) }}</button>
            </div>
          </article>
        </template>
      </section>

      <section v-if="closed.length" class="group">
        <button class="fold" @click="closedOpen = !closedOpen">{{ closedOpen ? '▾' : '▸' }} Settled, normal and fixed ({{ closed.length }})</button>
        <template v-if="closedOpen">
          <article v-for="f in closed" :key="f.id" class="finding closed">
            <div class="title"><span class="chip">{{ stateText[f.state] }}</span> {{ f.title }}</div>
            <div v-if="f.stateWhy" class="why">{{ f.stateWhy }}</div>
            <div v-if="findingButtons(f).length" class="buttons">
              <button v-for="word in findingButtons(f)" :key="word" class="quiet" :disabled="busy !== 0" @click="press(f, word)">{{ buttonText(f, word) }}</button>
            </div>
          </article>
        </template>
      </section>

      <section class="group">
        <button class="fold" @click="toggleLog">{{ logOpen ? '▾' : '▸' }} The patrol log</button>
        <pre v-if="logOpen" class="log">{{ log || 'No patrol yet.' }}</pre>
      </section>
    </template>
  </div>
</template>

<style scoped>
.sentinel {
  display: flex;
  flex-direction: column;
  gap: 16px;
  max-width: 820px;
}
.head {
  padding: 14px 16px;
  border: 1px solid var(--line);
  border-left: 4px solid var(--good);
  border-radius: 12px;
  background: var(--widget-background);
}
.head.look {
  border-left-color: var(--warn);
}
.head.alarm {
  border-left-color: var(--bad);
}
.head.unset,
.head.away {
  border-left-color: var(--line);
}
.line {
  margin: 0 0 10px;
  font-size: 15px;
}
.row {
  display: flex;
  gap: 12px;
  align-items: center;
  justify-content: space-between;
}
.when,
.seen {
  color: var(--text-dim);
  font-size: 12px;
}
.said {
  margin: 10px 0 0;
  color: var(--good);
}
.network {
  margin: 10px 0 0;
  color: var(--text-dim);
  font-size: 12.5px;
}
.problem {
  margin: 10px 0 0;
  color: var(--bad);
}
.group h3 {
  margin: 0 0 8px;
  font-size: 13px;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--text-dim);
}
.finding {
  margin-bottom: 8px;
  padding: 10px 14px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--widget-background);
}
.finding.alarm {
  border-color: color-mix(in srgb, var(--bad) 55%, var(--line));
}
.finding.look {
  border-color: color-mix(in srgb, var(--warn) 45%, var(--line));
}
.title {
  font-weight: 600;
}
.detail,
.why {
  margin-top: 4px;
  color: var(--text-dim);
  font-size: 13px;
  overflow-wrap: anywhere;
}
.buttons {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
  margin-top: 8px;
}
.calm {
  margin: 0;
  color: var(--text-dim);
}
.fold {
  background: none;
  border: none;
  padding: 4px 0;
  color: var(--text-dim);
  cursor: pointer;
}
.fold:hover {
  color: var(--text);
}
.chip {
  margin-right: 6px;
  padding: 1px 8px;
  border: 1px solid var(--line);
  border-radius: 999px;
  font-size: 11px;
  font-weight: 400;
  color: var(--text-dim);
}
.log {
  max-height: 320px;
  overflow: auto;
  padding: 10px 12px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--widget-background);
  font-size: 12px;
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
</style>
