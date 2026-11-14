<script setup>
import { computed, onMounted, ref, watch } from 'vue'
import { dbTraitActivate, dbTraitRetire, dbTraitSave, dbTraits, emotionPropose, getSettings } from '../../api/castle.js'
import ModalSheet from '../../components/ModalSheet.vue'
import ProfileCards from './ProfileCards.vue'
import ProfileConstellation from './ProfileConstellation.vue'
import { activeTraits, AUDIENCES, audienceOf, profileLookOf, traitNote, waitingItems } from './profile.js'

// HOME's PROFILE tab (Phase 49): the companion's emotion engine. The member scans the diary or
// writes an idea, the companion proposes, and nothing is kept without the member's yes. All the
// behaviour lives here; ProfileCards and ProfileConstellation only draw it (props in, events out).
const props = defineProps({
  shown: { type: Boolean, default: false },
  companion: { type: String, default: 'Your companion' },
})
const emit = defineEmits(['settings'])

const traits = ref([])
const fresh = ref([]) // proposals a scan or an idea just brought, not kept yet
const look = ref('cards')
const problem = ref('')
const noDatabase = ref(false)
const busy = ref('') // 'diary' | 'idea' | 'save'
const writing = ref(false)
const idea = ref('')
const open = ref(null) // { item, draft, editing, retireAsked }

const lived = computed(() => activeTraits(traits.value))
const waiting = computed(() => waitingItems(fresh.value, traits.value))

async function load() {
  try {
    look.value = profileLookOf((await getSettings()).settings?.profileLook)
  } catch {
    // outside Castle's window: the default look
  }
  try {
    traits.value = await dbTraits()
    problem.value = ''
    noDatabase.value = false
  } catch (reason) {
    const why = String(reason?.message ?? reason)
    noDatabase.value = /database/i.test(why)
    problem.value = why
  }
}

async function propose(kind) {
  if (busy.value) return
  busy.value = kind
  problem.value = ''
  try {
    const got = await emotionPropose(kind, kind === 'idea' ? idea.value : null)
    fresh.value = [...got.map((p) => ({ ...p, origin: kind === 'diary' ? 'diary' : 'written' })), ...fresh.value]
    if (kind === 'idea') {
      idea.value = ''
      writing.value = false
    }
  } catch (reason) {
    problem.value = String(reason?.message ?? reason)
  } finally {
    busy.value = ''
  }
}

async function keep(item) {
  busy.value = 'save'
  problem.value = ''
  try {
    if (item.source === 'kept') await dbTraitActivate(item.number)
    else {
      await dbTraitSave({ name: item.name, line: item.line, body: item.body ?? '', audience: item.audience, why: item.why ?? null }, null, item.origin ?? 'written')
      fresh.value = fresh.value.filter((_, i) => i !== item.index)
    }
    traits.value = await dbTraits()
  } catch (reason) {
    problem.value = String(reason?.message ?? reason)
  } finally {
    busy.value = ''
  }
}

async function no(item) {
  problem.value = ''
  try {
    if (item.source === 'kept') {
      await dbTraitRetire(item.number)
      traits.value = await dbTraits()
    } else fresh.value = fresh.value.filter((_, i) => i !== item.index)
  } catch (reason) {
    problem.value = String(reason?.message ?? reason)
  }
}

function show(item, editing = false) {
  open.value = { item, editing, retireAsked: false, draft: { name: item.name, line: item.line, body: item.body ?? '', audience: item.audience ?? 'everyone' } }
}

async function saveOpen() {
  const { item, draft } = open.value
  if (item.source === 'new') {
    // a proposal edited before the yes: kept as the member wrote it
    await keep({ ...item, ...draft })
    open.value = null
    return
  }
  busy.value = 'save'
  try {
    const saved = await dbTraitSave({ name: draft.name, line: draft.line, body: draft.body, audience: draft.audience }, item.number)
    if (item.source === 'kept') {
      // her own proposal, edited: Keep is the yes
      await dbTraitActivate(item.number)
      traits.value = await dbTraits()
      open.value = null
      return
    }
    traits.value = await dbTraits()
    open.value = { ...open.value, item: { ...item, ...saved }, editing: false }
  } catch (reason) {
    problem.value = String(reason?.message ?? reason)
  } finally {
    busy.value = ''
  }
}

async function setAudience(id) {
  open.value.draft.audience = id
  if (open.value.item.source === 'new' || open.value.editing) return
  await saveOpen()
}

// The constellation's pills: who hears a trait, changed in place (a new proposal only in the page).
async function audienceOfItem(item, id) {
  if (item.source === 'new') {
    fresh.value = fresh.value.map((p, i) => (i === item.index ? { ...p, audience: id } : p))
    return
  }
  try {
    await dbTraitSave({ audience: id }, item.number)
    traits.value = await dbTraits()
  } catch (reason) {
    problem.value = String(reason?.message ?? reason)
  }
}

async function retireOpen() {
  if (!open.value.retireAsked) {
    open.value.retireAsked = true
    return
  }
  try {
    await dbTraitRetire(open.value.item.number)
    traits.value = await dbTraits()
    open.value = null
  } catch (reason) {
    problem.value = String(reason?.message ?? reason)
  }
}

watch(
  () => props.shown,
  (on) => on && load(),
)
onMounted(() => props.shown && load())
</script>

<template>
  <section class="profile" :aria-label="`${companion}'s emotion engine`">
    <div v-if="noDatabase" class="invite">
      <p>{{ companion }}'s emotion engine lives in the memory database. Move your memory in first, then come back here.</p>
      <button class="primary" @click="emit('settings')">Open Settings &gt; Database</button>
    </div>

    <template v-else>
      <ProfileCards
        v-if="look === 'cards'"
        :companion="companion"
        :lived="lived"
        :waiting="waiting"
        :busy="busy"
        @open="show"
        @keep="keep"
        @edit="(i) => show(i, true)"
        @no="no"
        @scan="propose('diary')"
        @write="writing = !writing"
      />
      <ProfileConstellation
        v-else
        :companion="companion"
        :lived="lived"
        :waiting="waiting"
        :busy="busy"
        @open="show"
        @keep="keep"
        @edit="(i) => show(i, true)"
        @no="no"
        @audience="audienceOfItem"
        @scan="propose('diary')"
        @write="writing = !writing"
      />

      <form v-if="writing" class="idea" @submit.prevent="propose('idea')">
        <label for="trait-idea">A trait in your own words</label>
        <textarea id="trait-idea" v-model="idea" rows="2" maxlength="600" :placeholder="`she gets shy when praised...`" :disabled="!!busy"></textarea>
        <div class="row">
          <button class="primary" :disabled="!idea.trim() || !!busy">{{ busy === 'idea' ? 'Thinking…' : `Let ${companion} write it` }}</button>
          <button type="button" :disabled="!!busy" @click="writing = false">Cancel</button>
        </div>
      </form>
      <p v-if="busy === 'diary'" class="hint">{{ companion }} is reading your recent diary…</p>
      <p class="hint">Scanning and writing use your own Claude Code or Codex, once per press. Nothing is kept until you press Keep.</p>
      <p v-if="problem" class="problem" role="alert">{{ problem }}</p>
    </template>

    <ModalSheet v-if="open" :tag="open.item.number ? `TRAIT ${open.item.number}` : 'A NEW TRAIT'" :title="open.editing ? 'Change the words' : open.item.name" @close="open = null">
      <div class="sheet">
        <template v-if="open.editing">
          <label>Name <input v-model="open.draft.name" maxlength="80" /></label>
          <label>How it sounds (one line, may be heard in the room) <input v-model="open.draft.line" maxlength="200" /></label>
          <label>What it means <textarea v-model="open.draft.body" rows="5"></textarea></label>
        </template>
        <template v-else>
          <p class="sound">How it sounds: <em>{{ open.item.line }}</em></p>
          <p v-if="open.item.body" class="body">{{ open.item.body }}</p>
          <p v-if="open.item.why" class="why">Why (private): {{ open.item.why }}</p>
        </template>
        <h4>WHO HEARS IT IN THE ROOM</h4>
        <div class="pills" role="radiogroup">
          <button v-for="a in AUDIENCES" :key="a.id" type="button" role="radio" :aria-checked="open.draft.audience === a.id" :class="{ on: open.draft.audience === a.id }" :title="a.line" @click="setAudience(a.id)">{{ a.label }}</button>
        </div>
        <p class="hint">{{ audienceOf(open.draft.audience).line }}.</p>
        <p v-if="open.item.number" class="hint">{{ traitNote(open.item) }}</p>
        <div class="row">
          <template v-if="open.editing || open.item.source === 'new'">
            <button class="primary" :disabled="!open.draft.name.trim() || !open.draft.line.trim() || !!busy" @click="saveOpen">{{ open.item.source === 'new' || open.item.source === 'kept' ? 'Keep' : 'Save' }}</button>
            <button v-if="open.editing && open.item.source !== 'new'" @click="open.editing = false">Cancel</button>
            <button v-if="!open.editing" @click="open.editing = true">Edit</button>
          </template>
          <template v-else>
            <button @click="open.editing = true">Edit</button>
            <button v-if="open.item.status === 'active'" :class="{ danger: open.retireAsked }" @click="retireOpen">{{ open.retireAsked ? 'Press again to retire' : 'Retire' }}</button>
          </template>
        </div>
      </div>
    </ModalSheet>
  </section>
</template>

<style scoped>
.profile {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
  padding: 12px 4px;
  overflow-y: auto;
}
.invite {
  margin: auto;
  max-width: 340px;
  text-align: center;
  font-size: 14px;
}
.idea {
  display: flex;
  flex-direction: column;
  gap: 6px;
  padding: 10px 12px;
  border: 1px solid var(--line);
  border-radius: 10px;
}
.idea label,
h4 {
  font-size: 10.5px;
  letter-spacing: 0.2em;
  color: var(--text-dim);
}
textarea,
input {
  box-sizing: border-box;
  width: 100%;
  padding: 8px 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}
.row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
.hint {
  margin: 0;
  font-size: 12px;
  color: var(--text-dim);
}
.problem {
  margin: 0;
  font-size: 13px;
  color: var(--warn);
}
.sheet {
  display: flex;
  flex-direction: column;
  gap: 10px;
  max-width: 640px;
}
.sheet label {
  display: flex;
  flex-direction: column;
  gap: 4px;
  font-size: 12px;
  color: var(--text-dim);
}
.sound {
  font-size: 15px;
}
.why {
  font-size: 12.5px;
  color: var(--text-dim);
}
.pills {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}
.pills button {
  border-radius: 999px;
}
.pills button.on {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 30%, transparent);
  color: var(--text);
}
button {
  padding: 7px 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: transparent;
  color: var(--text);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}
button.primary {
  border-color: var(--accent);
  background: var(--accent);
  color: #07142e;
}
button.danger {
  border-color: var(--bad);
  color: var(--bad);
}
button:disabled {
  opacity: 0.45;
  cursor: default;
}
button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
</style>
