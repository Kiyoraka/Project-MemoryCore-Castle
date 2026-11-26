<script setup>
// STUDIO (designs studio-3 and studio-workflows-2): the four powers at the top. For Picture, the
// Workflows rail on the left, the chosen workflow's form in the middle and the canvas on the right
// (the newest picture large, the Made here strip, and the gallery behind its tabs). A workflow's
// picture goes to the queue (castle-image with the key, or Codex). Video, Song and Voice keep one
// Make box: they are asked of the companion, which runs its own helper and tells the cost. Paint is
// a page of its own (components/paint/PaintRoom.vue): one canvas the member and the companion share,
// kept mounted once opened so its strokes stay while another power is shown.
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { convertFileSrc } from '@tauri-apps/api/core'
import ImageViewer from '../components/ImageViewer.vue'
import ImageJobs from '../components/ImageJobs.vue'
import ThemedAudio from '../components/ThemedAudio.vue'
import WorkflowRail from '../components/WorkflowRail.vue'
import WorkflowForm from '../components/WorkflowForm.vue'
import WorkflowBuilder from '../components/WorkflowBuilder.vue'
import MarketplacePanel from '../components/MarketplacePanel.vue'
import PublishSheet from '../components/PublishSheet.vue'
import HelpTip from '../components/HelpTip.vue'
import PaintRoom from '../components/paint/PaintRoom.vue'
import { chatMediaOpen, imageRequest, studioDelete, voiceStudioSpeak, workflowDelete, workflowDuplicate, workflowExport, workflowFolder, workflowImport, workflowList, memberStatus } from '../api/castle.js'
import { refreshStudio, studioLib, studioPowersLoaded, studioPowersNow, studioThumbUrls, studioVoiceUp } from '../composables/useStudio.js'
import { askCompanion } from '../api/ask.js'
import { POWERS, companionLines, makeAction, powerPill } from '../components/studiomake.js'
import { companionBrief } from '../components/workflows.js'
import { publishAction, shopVisible } from '../components/shop.js'
import { isMac } from '../platform.js'

const props = defineProps({
  companion: { type: String, default: 'Your companion' },
  stamp: { type: Number, default: 0 },
  // STUDIO stays mounted (App.vue keeps it, so a painting's strokes survive): shown again, it reads anew
  visible: { type: Boolean, default: true },
})
const emit = defineEmits(['keys'])

// kept for the whole run and read at the start (composables/useStudio.js), so the room opens full
const powers = studioPowersNow
const voiceUp = studioVoiceUp
const lib = studioLib

// ---------- the Make bar ----------
const power = ref('image')
const powersLoaded = studioPowersLoaded
const brief = ref('')
const making = ref(false)
const makeProblem = ref('')
const makeDone = ref('')
const voiceClip = ref('')
const action = computed(() => makeAction(power.value, powers.value, voiceUp.value, props.companion, powersLoaded.value))
// PAINT is mounted the first time it is chosen and then kept (v-show), so its canvas never resets
const paintSeen = ref(false)
watch(power, (p) => {
  if (p === 'paint') paintSeen.value = true
})
const placeholder = computed(() => ({ image: 'Describe the picture…', video: 'Describe the short video…', song: 'What should the song be about…', voice: 'Type the words to say…' })[power.value])
const canMake = computed(() => action.value.kind === 'key' || (action.value.kind !== 'checking' && !!brief.value.trim() && !making.value))

async function make() {
  const a = action.value
  makeProblem.value = ''
  makeDone.value = ''
  if (a.kind === 'checking') return
  if (a.kind === 'key') return emit('keys')
  if (!brief.value.trim() || making.value) return
  making.value = true
  try {
    if (a.kind === 'image') {
      await imageRequest(brief.value)
    } else if (a.kind === 'voicestudio') {
      voiceClip.value = convertFileSrc(await voiceStudioSpeak(brief.value))
      await load()
    } else {
      await askCompanion(companionLines(a.ask, brief.value), props.companion)
      makeDone.value = `Asked ${props.companion} - it tells you the cost in Chat and asks before it makes it.`
    }
    brief.value = ''
  } catch (e) {
    makeProblem.value = String(e?.message ?? e)
  } finally {
    making.value = false
  }
}

// ---------- the workflows (Picture) ----------
const entries = ref([])
const selected = ref('')
const current = computed(() => entries.value.find((e) => e.id === selected.value) ?? null)
const listProblem = ref('')
async function loadWorkflows() {
  try {
    entries.value = await workflowList()
    listProblem.value = ''
    if (!entries.value.some((e) => e.id === selected.value)) selected.value = entries.value.find((e) => !e.error)?.id ?? ''
  } catch (e) {
    listProblem.value = String(e?.message ?? e)
  }
}

// ---------- make your own: New, Edit, Duplicate ----------
// builder: { start, replace } while the builder is open
const builder = ref(null)
const railProblem = ref('')
const openNew = () => (builder.value = { start: null, replace: '' })
const openEdit = (entry) => (builder.value = { start: entry.workflow, replace: entry.id })
async function duplicate(entry) {
  railProblem.value = ''
  try {
    const id = await workflowDuplicate(entry.id)
    await loadWorkflows()
    selected.value = id
    const copy = entries.value.find((e) => e.id === id)
    if (copy) openEdit(copy)
  } catch (e) {
    railProblem.value = String(e?.message ?? e)
  }
}
// Ask the companion: a brief through the session, then the rail looks for the new file a while
const askWords = ref('')
const askLine = ref('')
let askTimer
async function askForWorkflow() {
  railProblem.value = ''
  askLine.value = ''
  if (!askWords.value.trim()) return
  try {
    const folder = await workflowFolder()
    await askCompanion(companionBrief(askWords.value, folder), props.companion)
    askWords.value = ''
    askLine.value = `Asked ${props.companion}. The new workflow appears here when it is written.`
    const before = new Set(entries.value.map((e) => e.id))
    clearInterval(askTimer)
    const until = Date.now() + 10 * 60 * 1000
    askTimer = setInterval(async () => {
      await loadWorkflows()
      const made = entries.value.find((e) => !before.has(e.id))
      if (made || Date.now() > until) {
        clearInterval(askTimer)
        if (made) {
          selected.value = made.id
          askLine.value = made.error ? `${made.workflow?.name ?? made.id} arrived with a problem: ${made.error}` : `${made.workflow?.name ?? made.id} is ready.`
        }
      }
    }, 4000)
  } catch (e) {
    railProblem.value = String(e?.message ?? e)
  }
}
onBeforeUnmount(() => clearInterval(askTimer))

// Export, Import, Delete (Delete asks once, in place)
const railLine = ref('')
const askingDelete = ref('')
async function railAct(work) {
  railProblem.value = ''
  railLine.value = ''
  try {
    await work()
  } catch (e) {
    railProblem.value = String(e?.message ?? e)
  }
}
const exportCurrent = (entry) =>
  railAct(async () => {
    const path = await workflowExport(entry.id)
    if (path) railLine.value = `Saved to ${path}`
  })
const importOne = () =>
  railAct(async () => {
    const id = await workflowImport()
    if (!id) return
    await loadWorkflows()
    selected.value = id
    railLine.value = 'Imported. It is yours now.'
  })
const deleteCurrent = (entry) =>
  railAct(async () => {
    await workflowDelete(entry.id)
    askingDelete.value = ''
    selected.value = ''
    await loadWorkflows()
  })

async function saved(id) {
  builder.value = null
  await loadWorkflows()
  selected.value = id
}

// ---------- the Marketplace (hidden until the Portfolio's doors are live) ----------
const member = ref(null)
const side = ref('workflows') // workflows | marketplace
const showShop = computed(() => shopVisible(member.value))
const publishing = ref(null)
async function loadMember() {
  try {
    member.value = await memberStatus()
  } catch {
    member.value = null
  }
  if (!showShop.value) side.value = 'workflows'
}
async function published() {
  publishing.value = null
  railLine.value = 'Sent for review. You will see it in Marketplace > My listings.'
}

// ---------- the canvas and the gallery ----------
const TABS = [
  ['images', 'Pictures'],
  ['videos', 'Videos'],
  ['songs', 'Songs'],
  ['voices', 'Voice'],
  ['saved', 'Saved prompts'],
  ['backups', 'Backups'],
]
const tab = ref('latest')
// Latest belongs to Picture's canvas; the other powers open on the shelf of their own kind
watch(power, (p) => {
  asking.value = ''
  if (p === 'paint') return
  if (p === 'image') tab.value = 'latest'
  else if (tab.value === 'latest') tab.value = { video: 'videos', song: 'songs', voice: 'voices' }[p] ?? 'images'
})
const latest = computed(() => lib.value.images?.[0] ?? null)
const strip = computed(() => (lib.value.images ?? []).slice(0, 12))
const viewerStart = ref(null)
const src = (path) => convertFileSrc(path)
const counts = computed(() => Object.fromEntries(TABS.map(([t]) => [t, lib.value[t]?.length ?? 0])))
// the viewer walks the shelf in view: Pictures, or the Backups Castle kept
const shelfPictures = computed(() => (tab.value === 'backups' ? lib.value.backups : lib.value.images))
const openPicture = (path) => {
  const i = (lib.value.images ?? []).findIndex((m) => m.path === path)
  if (i < 0) return
  if (tab.value === 'backups') tab.value = 'latest'
  viewerStart.value = i
}
const pictures = computed(() => shelfPictures.value.map((m) => ({ path: m.path, name: m.name, url: src(m.path) })))

// Delete asks once, in place ("Delete? Yes / No"), then sends the file to the Recycle Bin.
const asking = ref('')
const deleteProblem = ref('')
async function remove(path) {
  deleteProblem.value = ''
  try {
    await studioDelete(path)
    asking.value = ''
    await load()
  } catch (e) {
    deleteProblem.value = String(e?.message ?? e)
  }
}

function when(at) {
  return at ? new Date(at * 1000).toLocaleString('en-US', { dateStyle: 'medium', timeStyle: 'short' }) : ''
}

// what is already read shows at once; this reads again quietly and swaps in anything new
const load = refreshStudio

onMounted(() => {
  load()
  loadWorkflows()
  loadMember()
})
watch(() => props.stamp, () => {
  load()
  loadWorkflows()
})
watch(() => props.visible, (shown) => {
  if (!shown) return
  load()
  loadWorkflows()
})
</script>

<template>
  <div class="studio">
    <header class="top widget">
      <h3>STUDIO</h3>
      <span class="spacer"></span>
      <ImageJobs @finished="load" />
      <div class="switch" role="radiogroup" aria-label="What to make">
        <button
          v-for="p in POWERS"
          :key="p.id"
          role="radio"
          :aria-checked="power === p.id"
          :class="{ on: power === p.id }"
          @click="power = p.id"
        >
          <span class="power-name">{{ p.label }}</span>
          <span class="pill" :class="powerPill(p.id, powers, voiceUp, powersLoaded).tone">{{ powerPill(p.id, powers, voiceUp, powersLoaded).text }}</span>
        </button>
      </div>
    </header>

    <div class="body" :class="{ maker: power === 'image' }">
      <template v-if="power === 'image'">
        <section class="rail-box widget">
          <nav v-if="showShop" class="sides">
            <button :class="{ active: side === 'workflows' }" @click="side = 'workflows'">Workflows</button>
            <button :class="{ active: side === 'marketplace' }" @click="side = 'marketplace'">Marketplace</button>
          </nav>
          <WorkflowRail :entries="entries" :selected="selected" @select="selected = $event">
            <button class="new-card" @click="openNew">＋ New workflow</button>
            <button class="import" @click="importOne">Import a workflow file</button>
            <div class="ask">
              <textarea v-model="askWords" rows="2" maxlength="600" :placeholder="`Or describe one for ${companion}…`" aria-label="Describe a workflow for your companion"></textarea>
              <button :disabled="!askWords.trim()" @click="askForWorkflow">Ask {{ companion }}</button>
              <p v-if="askLine" class="hint">{{ askLine }}</p>
            </div>
          </WorkflowRail>
          <p v-if="listProblem || railProblem" role="alert" class="problem">{{ listProblem || railProblem }}</p>
          <p v-else-if="railLine" class="hint">{{ railLine }}</p>
        </section>
        <MarketplacePanel v-if="side === 'marketplace'" class="shop-span" :status="member" :entries="entries" @installed="loadWorkflows" />
        <section v-else class="form-box widget">
          <div v-if="current" class="card-actions">
            <button v-if="current.source === 'yours' && !current.error" @click="openEdit(current)">Edit</button>
            <button v-if="!current.error" @click="duplicate(current)">Duplicate</button>
            <button v-if="current.source === 'yours' && !current.error" @click="exportCurrent(current)">Export</button>
            <button v-if="publishAction(member, current).show" :disabled="!publishAction(member, current).enabled" @click="publishing = current">{{ publishAction(member, current).label }}</button>
            <template v-if="current.source !== 'built-in'">
              <span v-if="askingDelete === current.id" class="inline-confirm">Delete? <button class="danger" @click="deleteCurrent(current)">Yes</button> <button @click="askingDelete = ''">No</button></span>
              <button v-else class="text-delete" @click="askingDelete = current.id">Delete</button>
            </template>
          </div>
          <WorkflowForm v-if="current && !current.error" :entry="current" :way="powers.image" :comfy="!!powers.comfy" :loaded="powersLoaded" @keys="emit('keys')" />
          <p v-else-if="current" role="alert" class="problem">This workflow has a problem: {{ current.error }}</p>
          <p v-else class="empty">Choose a workflow on the left.</p>
        </section>
      </template>

      <section v-else-if="power !== 'paint'" class="make widget">
        <h3>MAKE</h3>
        <div class="make-row">
          <textarea v-model="brief" rows="2" :placeholder="placeholder" aria-label="What to make" :disabled="action.kind === 'key'" @keydown.ctrl.enter="make" @keydown.meta.enter="isMac && make()"></textarea>
          <button class="primary create" :disabled="!canMake" @click="make">{{ making ? 'Working…' : action.label }}</button>
        </div>
        <p v-if="makeProblem" role="alert" class="problem">{{ makeProblem }}</p>
        <p v-else-if="makeDone" class="hint">{{ makeDone }}</p>
        <ThemedAudio v-if="voiceClip && power === 'voice'" :src="voiceClip" />
      </section>

      <PaintRoom v-if="paintSeen" v-show="power === 'paint'" :companion="companion" />

      <section v-if="power !== 'paint' && (power !== 'image' || side !== 'marketplace')" class="gallery widget">
        <nav class="tabs has-tip">
          <button v-if="power === 'image'" :class="{ active: tab === 'latest' }" @click="tab = 'latest'; asking = ''">Latest</button>
          <button v-for="[t, label] in TABS" :key="t" :class="{ active: tab === t }" @click="tab = t; asking = ''">{{ label }} ({{ counts[t] }})</button>
          <HelpTip v-if="tab === 'backups'" block text="Castle keeps the picture it replaces when it draws or you upload a new one. Delete the ones you no longer need; they go to the Recycle Bin." />
          <HelpTip v-else-if="tab === 'saved'" block text="These were saved instead of made, because a key was missing. Add the key and make them again." />
          <span class="spacer"></span>
          <button @click="load">Refresh</button>
        </nav>

        <div v-if="tab === 'latest'" class="latest">
          <button v-if="latest" class="hero" :title="latest.name" @click="openPicture(latest.path)">
            <img :src="src(latest.path)" :alt="latest.name" />
          </button>
          <p v-else class="empty">Your pictures appear here. Choose a workflow and press Create picture.</p>
          <div v-if="strip.length" class="strip-box">
            <h4>Made here</h4>
            <div class="strip">
              <button v-for="m in strip" :key="m.path" class="strip-thumb" :class="{ on: latest && m.path === latest.path }" :title="m.name" @click="openPicture(m.path)">
                <img :src="studioThumbUrls[m.path] ?? src(m.path)" :alt="m.name" loading="lazy" />
              </button>
            </div>
          </div>
        </div>

        <template v-else>
          <p v-if="deleteProblem" role="alert" class="problem">{{ deleteProblem }}</p>

          <div class="shelf">
            <div v-if="tab === 'images' || tab === 'backups'" class="masonry">
              <div v-for="(m, i) in shelfPictures" :key="m.path" class="tile">
                <button class="thumb" :title="m.name" @click="viewerStart = i">
                  <img :src="studioThumbUrls[m.path] ?? src(m.path)" :alt="m.name" loading="lazy" />
                </button>
                <div v-if="asking === m.path" class="confirm" role="group" aria-label="Delete this picture">
                  <span>Delete?</span>
                  <button class="danger" @click="remove(m.path)">Yes</button>
                  <button @click="asking = ''">No</button>
                </div>
                <button v-else class="delete" :title="`Delete ${m.name}`" :aria-label="`Delete ${m.name}`" @click="asking = m.path">✕</button>
              </div>
            </div>
            <div v-else class="rows">
              <div v-for="m in lib[tab]" :key="m.path" class="row" :class="tab">
                <video v-if="tab === 'videos'" :src="src(m.path)" controls preload="metadata"></video>
                <div class="row-name">
                  <strong>{{ m.name }}</strong>
                  <span v-if="m.group" class="group">{{ m.group }}</span>
                  <span class="group">{{ when(m.at) }}</span>
                </div>
                <ThemedAudio v-if="tab === 'songs' || tab === 'voices'" class="clip" :src="src(m.path)" />
                <button v-if="tab === 'saved'" @click="chatMediaOpen(m.path, true)">Show file</button>
                <span v-if="asking === m.path" class="inline-confirm">Delete? <button class="danger" @click="remove(m.path)">Yes</button> <button @click="asking = ''">No</button></span>
                <button v-else class="text-delete" @click="asking = m.path">Delete</button>
              </div>
            </div>
            <p v-if="!counts[tab]" class="empty">Nothing here yet.</p>
          </div>
        </template>
      </section>
    </div>

    <PublishSheet v-if="publishing" :entry="publishing" @close="publishing = null" @published="published" />
    <WorkflowBuilder v-if="builder" :start="builder.start" :replace="builder.replace" @saved="saved" @close="builder = null" />
    <ImageViewer v-if="viewerStart !== null" :pictures="pictures" :start="viewerStart" @close="viewerStart = null" />
  </div>
</template>

<style scoped>
.studio {
  position: absolute;
  inset: 24px;
  display: flex;
  flex-direction: column;
  gap: 16px;
}
.widget {
  background: var(--panel-background);
  border: 1px solid var(--line);
  border-radius: 14px;
  -webkit-backdrop-filter: blur(var(--glass-blur)); backdrop-filter: blur(var(--glass-blur));
}
h3 {
  margin: 0;
  font-size: 13px;
  font-weight: 500;
  letter-spacing: 0.18em;
}

/* the top bar: the room's name, the request line and the four powers */
.top {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 10px 16px;
  flex-shrink: 0;
}
.body {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 16px;
  min-height: 0;
}
/* Picture: the rail, the form and the canvas side by side */
.body.maker {
  display: grid;
  grid-template-columns: minmax(200px, 240px) minmax(300px, 380px) 1fr;
}
.rail-box,
.form-box {
  display: flex;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
  padding: 14px;
}
.latest {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 12px;
  min-height: 0;
}
.hero {
  flex: 1;
  min-height: 0;
  padding: 0;
  overflow: hidden;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: rgba(0, 0, 0, 0.2);
  cursor: zoom-in;
}
.hero img {
  display: block;
  width: 100%;
  height: 100%;
  object-fit: contain;
}
.strip-box h4 {
  margin: 0 0 6px;
  font-size: 13px;
  font-weight: 500;
  color: var(--text-dim);
}
.strip {
  display: flex;
  gap: 10px;
  overflow-x: auto;
  scrollbar-width: var(--scrollbar-width);
  scrollbar-color: var(--scrollbar-thumb) var(--scrollbar-track);
}
.strip-thumb {
  width: 120px;
  height: 84px;
  flex-shrink: 0;
  padding: 0;
  overflow: hidden;
  border-radius: 10px;
}
.strip-thumb.on {
  border-color: var(--accent);
  box-shadow: 0 0 0 2px var(--accent);
}
.strip-thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.sides {
  display: flex;
  gap: 6px;
}
.sides button {
  flex: 1;
  border-radius: 999px;
}
.sides .active {
  border-color: var(--accent);
  background: var(--accent);
  color: #07142e;
  font-weight: 600;
}
.shop-span {
  grid-column: 2 / 4;
}
.new-card {
  padding: 10px 12px;
  border-style: dashed;
  border-radius: 12px;
  font-size: 14px;
  text-align: left;
}
.import {
  padding: 6px 12px;
  font-size: 13px;
  color: var(--text-dim);
}
.ask {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-top: 6px;
}
.ask textarea {
  width: 100%;
  box-sizing: border-box;
  padding: 8px 10px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 13px;
  resize: vertical;
}
.card-actions {
  display: flex;
  flex-wrap: wrap;
  justify-content: flex-end;
  gap: 6px;
}

/* the Make box (Video, Song, Voice) */
.make {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 16px 20px;
  flex-shrink: 0;
}
.make-row {
  display: flex;
  flex-wrap: wrap;
  align-items: stretch;
  gap: 14px;
}
.make-row .create {
  /* the label changes with the power and what it can do (Create picture, Ask <companion>, Add the
     ElevenLabs key, Working...): a fixed floor keeps the button, and the box beside it, from
     shifting width as you switch powers or it starts working */
  flex: 0 0 auto;
  min-width: 190px;
  white-space: nowrap;
}
.make-row textarea {
  /* fills what the fixed-width switch and create button leave behind, so it keeps one width as
     you move between Picture, Video, Song and Voice instead of resizing with them */
  flex: 1 1 320px;
  min-width: 260px;
  min-height: 56px;
  resize: vertical;
  padding: 10px 12px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 14px;
}
.switch {
  display: flex;
  overflow: hidden;
  border: 1px solid var(--line);
  border-radius: 10px;
}
.switch button {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  gap: 4px;
  /* a fixed width, not a minimum: the pill below (Ready, Codex, VoiceStudio, Add a key) is a
     different length for each power, and a minimum let the button - and everything to its right -
     change width as you switched between them */
  width: 112px;
  flex-shrink: 0;
  padding: 8px 6px;
  border: 0;
  border-right: 1px solid var(--line);
  border-radius: 0;
}
.switch button:last-child {
  border-right: 0;
}
.switch button.on {
  background: color-mix(in srgb, var(--accent) 85%, transparent);
  color: #07142e;
}
.power-name {
  font-size: 14px;
  letter-spacing: 0.04em;
  white-space: nowrap;
}
.pill {
  padding: 1px 8px;
  border-radius: 999px;
  font-size: 10px;
  white-space: nowrap;
}
.pill.ready {
  background: color-mix(in srgb, var(--good) 30%, transparent);
  color: var(--good);
}
.switch button.on .pill.ready {
  background: #1f8f5f;
  color: #eafff4;
}
.pill.missing {
  background: color-mix(in srgb, var(--warn) 30%, transparent);
  color: var(--warn);
}
.switch button.on .pill.missing {
  background: #b07a1f;
  color: #fff6e6;
}
/* Picture's sign-in check still answering: neutral, not the "missing" colour, so a brief check
   never looks like a warning that something is actually wrong */
.pill.checking {
  background: color-mix(in srgb, var(--text-dim) 25%, transparent);
  color: var(--text-dim);
}
.switch button.on .pill.checking {
  background: color-mix(in srgb, #07142e 70%, transparent);
  color: #e8ecf4;
}
.create {
  min-width: 150px;
  font-size: 15px;
}

/* the gallery */
.gallery {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 10px;
  min-height: 0;
  padding: 14px 18px;
}
.tabs {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}
.tabs > button {
  border-radius: 999px;
  padding: 6px 16px;
}
.tabs .active {
  border-color: var(--accent);
  background: var(--accent);
  color: #07142e;
  font-weight: 600;
}
.spacer {
  flex: 1;
}
/* a row with a (?) whose bubble spans the row (HelpTip block) */
.has-tip {
  position: relative;
}
.shelf {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  scrollbar-width: var(--scrollbar-width);
  scrollbar-color: var(--scrollbar-thumb) var(--scrollbar-track);
}
.masonry {
  columns: 280px;
  column-gap: 12px;
}
.tile {
  position: relative;
  break-inside: avoid;
  margin-bottom: 12px;
}
.thumb {
  display: block;
  width: 100%;
  padding: 0;
  overflow: hidden;
  border: 1px solid var(--line);
  border-radius: 12px;
  cursor: zoom-in;
}
.thumb img {
  display: block;
  width: 100%;
  height: auto;
}
.delete {
  position: absolute;
  top: 8px;
  right: 8px;
  width: 28px;
  height: 28px;
  padding: 0;
  border-radius: 50%;
  border: 1px solid var(--line);
  background: color-mix(in srgb, var(--panel-background) 85%, transparent);
  color: var(--text);
  opacity: 0;
  cursor: pointer;
}
.tile:hover .delete,
.delete:focus-visible {
  opacity: 1;
}
.confirm {
  position: absolute;
  inset: auto 8px 8px 8px;
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 6px;
  padding: 6px;
  border-radius: 8px;
  background: color-mix(in srgb, var(--panel-background) 92%, transparent);
  font-size: 13px;
}
.danger {
  border-color: color-mix(in srgb, #ff6b81 60%, var(--line));
  color: #ff9aa9;
}
.rows {
  display: flex;
  flex-direction: column;
  gap: 10px;
}
.row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 12px;
  padding: 10px 14px;
  border: 1px solid var(--line);
  border-radius: 12px;
  background: color-mix(in srgb, var(--panel-background) 70%, transparent);
  font-size: 14px;
}
.row.videos video {
  width: 100%;
  max-width: 560px;
  border-radius: 10px;
}
.row-name {
  flex: 1;
  min-width: 180px;
}
.group {
  margin-left: 10px;
  font-size: 12px;
  color: var(--text-dim);
}
.inline-confirm {
  font-size: 13px;
}
.problem {
  margin: 0;
  font-size: 13px;
  color: var(--warn);
}
.hint,
.empty {
  margin: 0;
  font-size: 13px;
  color: var(--text-dim);
}
button {
  padding: 6px 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}
button.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: #07142e;
  font-weight: 600;
}
button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}
</style>
