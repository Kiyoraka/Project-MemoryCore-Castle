<script setup>
import { computed, onBeforeUnmount, onMounted, ref } from 'vue'
import { downloadAdd, downloadCancel, downloadOpen, downloadPause, downloadRemove, downloadResume, downloadShow, downloadsList, onDownloadsState, videoAdd, videoFormats, videoFromPage, videoToolGet, videoToolState } from '../api/castle.js'
import { dlCard, dlFilter, isVideoLink } from './browserview.js'

// The downloader (ported from the Alice Keep, browser-downloads-2): a glass panel on the right with the
// paste field, the category pills, a card per download and the video quality chooser. Every download a
// page starts lands here by itself. The pages step aside while it is open (BrowserWindow's overlay).
const props = defineProps({
  // a video page's address handed over by its Download video button, with its tab (for Threads' own video)
  start: { type: Object, default: null },
})
const emit = defineEmits(['close'])

const items = ref([])
const link = ref(props.start?.url ?? '')
const category = ref('All')
const problem = ref('')
const tool = ref({ installed: true, ffmpeg: true })
const chooser = ref(null) // { url, title, choices, pick, tab }
const busy = ref(false)
let unlisten = null

const CATEGORIES = ['All', 'Video', 'Music', 'Documents', 'Programs']
const shown = computed(() => dlFilter(items.value, category.value))

async function add() {
  const url = link.value.trim()
  problem.value = ''
  if (!url) return
  if (isVideoLink(url)) return askVideo(url, props.start?.tab ?? 0)
  try {
    await downloadAdd(url)
    link.value = ''
  } catch (reason) {
    problem.value = String(reason)
  }
}

async function askVideo(url, tab) {
  busy.value = true
  try {
    tool.value = await videoToolState()
    if (!tool.value.installed) return
    const f = await videoFormats(url)
    chooser.value = { url, title: f.title, choices: f.choices, pick: f.pick, tab }
  } catch (reason) {
    problem.value = String(reason)
    // Threads' own video, when the tool cannot read the post
    if (tab && /threads\.(com|net)/.test(url)) chooser.value = { url, title: '', choices: [], pick: '', tab, page: true }
  } finally {
    busy.value = false
  }
}

async function getTool() {
  busy.value = true
  problem.value = ''
  try {
    tool.value = await videoToolGet()
    if (link.value.trim()) await askVideo(link.value.trim(), props.start?.tab ?? 0)
  } catch (reason) {
    problem.value = String(reason)
  } finally {
    busy.value = false
  }
}

async function startVideo() {
  const c = chooser.value
  if (!c) return
  try {
    await videoAdd(c.url, c.pick, c.title || 'video')
    chooser.value = null
    link.value = ''
  } catch (reason) {
    problem.value = String(reason)
  }
}

async function pageVideo() {
  try {
    await videoFromPage(chooser.value.tab)
    chooser.value = null
    problem.value = ''
  } catch (reason) {
    problem.value = String(reason)
  }
}

const act = (fn, id) => fn(id).catch((reason) => (problem.value = String(reason)))

onMounted(async () => {
  items.value = await downloadsList().catch(() => [])
  unlisten = await onDownloadsState((list) => (items.value = list))
  tool.value = await videoToolState().catch(() => tool.value)
  if (props.start?.url && isVideoLink(props.start.url)) askVideo(props.start.url, props.start.tab ?? 0)
})
onBeforeUnmount(() => unlisten?.())
</script>

<template>
  <aside class="dl-panel" role="dialog" aria-label="Downloads">
    <header>
      <svg class="head-icon" viewBox="0 0 24 24" aria-hidden="true"><circle cx="12" cy="12" r="9" /><path d="M12 7v8m-4-4 4 4 4-4" /></svg>
      <h2>Downloads</h2>
      <button class="close" aria-label="Close" @click="emit('close')">&times;</button>
    </header>

    <div class="paste">
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1" /></svg>
      <input v-model="link" spellcheck="false" placeholder="Paste a link (YouTube, Threads or a file)" @keydown.enter="add" />
      <button class="go" :disabled="busy" @click="add">
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M12 5v10m-4-4 4 4 4-4M5 19h14" /></svg>
        Download
      </button>
    </div>
    <p v-if="!tool.installed" class="tool">
      Videos need the video tool (yt-dlp), fetched once from its own release and checked before it is kept.
      <button :disabled="busy" @click="getTool">{{ busy ? 'Getting it…' : 'Get the video tool' }}</button>
    </p>
    <p v-else-if="!tool.ffmpeg" class="tool">ffmpeg was not found: videos need it to join the picture and the sound.</p>
    <p v-if="problem" class="problem">{{ problem }}</p>

    <nav class="pills">
      <button v-for="c in CATEGORIES" :key="c" :class="{ on: category === c }" @click="category = c">{{ c }}</button>
    </nav>

    <div class="cards">
      <p v-if="!shown.length" class="empty">Nothing here yet. Downloads from any page land here by themselves.</p>
      <article v-for="d in shown" :key="d.id" class="card" :class="dlCard(d).colour">
        <span class="icon" :class="dlCard(d).icon" aria-hidden="true">
          <svg v-if="dlCard(d).icon === 'film'" viewBox="0 0 24 24"><rect x="3" y="5" width="18" height="14" rx="2" /><path d="M7 5v14M17 5v14M3 9h4M3 15h4M17 9h4M17 15h4" /></svg>
          <svg v-else-if="dlCard(d).icon === 'note'" viewBox="0 0 24 24"><path d="M9 18V6l10-2v12" /><circle cx="6.5" cy="18" r="2.5" /><circle cx="16.5" cy="16" r="2.5" /></svg>
          <svg v-else-if="dlCard(d).icon === 'box'" viewBox="0 0 24 24"><path d="M3 8l9-5 9 5v8l-9 5-9-5zM3 8l9 5 9-5M12 13v8" /></svg>
          <svg v-else viewBox="0 0 24 24"><path d="M6 3h8l4 4v14H6zM14 3v4h4M9 12h6M9 16h6" /></svg>
        </span>
        <div class="body">
          <b class="name" :title="d.path || d.url">{{ d.name }}</b>
          <small class="cat">{{ d.category }}</small>
          <div class="bar"><span :style="{ width: `${dlCard(d).percent}%` }" /></div>
          <small class="line">{{ dlCard(d).line }}</small>
        </div>
        <span class="pct">{{ d.state === 'queued' ? '' : `${dlCard(d).percent}%` }}</span>
        <div class="buttons">
          <button v-if="d.state === 'running' && d.pausable" title="Pause" @click="act(downloadPause, d.id)">
            <svg viewBox="0 0 24 24"><path d="M9 6v12M15 6v12" /></svg>
          </button>
          <button v-if="d.state === 'paused' || d.state === 'failed'" title="Resume" @click="act(downloadResume, d.id)">
            <svg viewBox="0 0 24 24"><path d="M8 5l11 7-11 7z" /></svg>
          </button>
          <button v-if="d.state === 'queued'" title="Queued" disabled>
            <svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="8" /><path d="M12 8v4l3 2" /></svg>
          </button>
          <button v-if="d.state === 'done'" class="text" @click="act(downloadOpen, d.id)">Open</button>
          <button v-if="d.path" title="Show in folder" @click="act(downloadShow, d.id)">
            <svg viewBox="0 0 24 24"><path d="M3 6h6l2 2h10v11H3z" /></svg>
          </button>
          <button v-if="d.state !== 'done'" title="Cancel" @click="act(downloadCancel, d.id)">
            <svg viewBox="0 0 24 24"><path d="M6 6l12 12M18 6 6 18" /></svg>
          </button>
          <button v-else title="Remove from the list (the file stays)" @click="act(downloadRemove, d.id)">
            <svg viewBox="0 0 24 24"><path d="M6 6l12 12M18 6 6 18" /></svg>
          </button>
        </div>
      </article>
    </div>

    <div v-if="chooser" class="chooser">
      <header>
        <svg viewBox="0 0 24 24" aria-hidden="true"><rect x="3" y="5" width="18" height="14" rx="2" /><path d="M10 9l5 3-5 3z" /></svg>
        <b>Video quality</b>
        <button class="close" aria-label="Close" @click="chooser = null">&times;</button>
      </header>
      <p v-if="chooser.title" class="title">{{ chooser.title }}</p>
      <div v-if="chooser.choices.length" class="choices">
        <button v-for="c in chooser.choices" :key="c" :class="{ on: chooser.pick === c }" @click="chooser.pick = c">{{ c }}</button>
      </div>
      <button v-if="chooser.choices.length" class="start" @click="startVideo">Start</button>
      <button v-else-if="chooser.page" class="start" @click="pageVideo">Take the page's own video</button>
    </div>
    <small class="terms">For your own videos, ones you have the right to keep, and offline viewing. DRM-protected video is never taken.</small>
  </aside>
</template>

<style scoped>
.dl-panel {
  position: absolute;
  z-index: 30;
  top: 54px;
  right: 14px;
  bottom: 14px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  width: min(560px, calc(100% - 28px));
  padding: 18px;
  border: 1px solid rgba(168, 140, 255, 0.35);
  border-radius: 16px;
  background: rgba(18, 14, 46, 0.96);
  backdrop-filter: blur(14px);
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.55);
  color: var(--text);
  overflow: hidden;
}

.dl-panel > header {
  display: flex;
  gap: 10px;
  align-items: center;
}

.dl-panel h2 {
  flex: 1;
  margin: 0;
  font-size: 22px;
  font-weight: 500;
}

.head-icon,
.paste svg,
.go svg,
.chooser header svg {
  width: 22px;
  height: 22px;
  fill: none;
  stroke: currentColor;
  stroke-width: 1.8;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.close {
  border: none;
  background: none;
  color: var(--text-dim, #b9b0d8);
  font-size: 22px;
  cursor: pointer;
}

.paste {
  display: flex;
  gap: 10px;
  align-items: center;
  padding: 6px 6px 6px 12px;
  border: 1px solid rgba(168, 140, 255, 0.3);
  border-radius: 12px;
  background: rgba(0, 0, 0, 0.25);
}

.paste input {
  flex: 1;
  min-width: 0;
  border: none;
  background: none;
  color: var(--text);
  font: inherit;
  outline: none;
}

.go {
  display: inline-flex;
  gap: 6px;
  align-items: center;
  padding: 8px 16px;
  border: none;
  border-radius: 10px;
  background: linear-gradient(135deg, #8a6cf0, #a98bff);
  color: #fff;
  font: inherit;
  cursor: pointer;
}

.go svg {
  width: 18px;
  height: 18px;
}

.tool,
.problem {
  margin: 0;
  font-size: 13px;
}

.tool button {
  margin-left: 8px;
  padding: 4px 12px;
  border: 1px solid rgba(126, 224, 180, 0.6);
  border-radius: 8px;
  background: rgba(126, 224, 180, 0.15);
  color: var(--text);
  cursor: pointer;
}

.problem {
  color: #ff9aa6;
}

.pills {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.pills button {
  padding: 6px 16px;
  border: 1px solid rgba(168, 140, 255, 0.3);
  border-radius: 999px;
  background: rgba(40, 30, 90, 0.5);
  color: var(--text);
  font: inherit;
  cursor: pointer;
}

.pills button.on {
  border-color: transparent;
  background: linear-gradient(135deg, #8a6cf0, #a98bff);
}

.cards {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 10px;
  overflow-y: auto;
}

.empty {
  margin: 20px 0;
  text-align: center;
  color: var(--text-dim, #b9b0d8);
}

.card {
  display: flex;
  gap: 14px;
  align-items: center;
  padding: 14px;
  border: 1px solid rgba(255, 255, 255, 0.08);
  border-radius: 14px;
  background: rgba(30, 24, 66, 0.75);
}

.card.gold {
  border-color: rgba(230, 194, 122, 0.45);
  background: rgba(60, 46, 30, 0.55);
}

.icon {
  display: grid;
  flex: none;
  place-items: center;
  width: 54px;
  height: 54px;
  border-radius: 12px;
  background: linear-gradient(135deg, #6c52d8, #9a7cff);
}

.icon.box {
  background: linear-gradient(135deg, #c99a2e, #f2c860);
}

.icon.doc,
.icon.file {
  background: linear-gradient(135deg, #3a6fd8, #66a0ff);
}

.icon svg {
  width: 28px;
  height: 28px;
  fill: none;
  stroke: #fff;
  stroke-width: 1.7;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.body {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 4px;
  min-width: 0;
}

.name {
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  font-weight: 500;
}

.cat,
.line {
  color: var(--text-dim, #b9b0d8);
}

.bar {
  height: 7px;
  margin-top: 4px;
  border-radius: 4px;
  background: rgba(255, 255, 255, 0.1);
  overflow: hidden;
}

.bar span {
  display: block;
  height: 100%;
  border-radius: 4px;
  transition: width 0.4s;
}

.mint .bar span {
  background: #7ee0b4;
  box-shadow: 0 0 10px rgba(126, 224, 180, 0.7);
}

.amber .bar span {
  background: #f5b452;
  box-shadow: 0 0 10px rgba(245, 180, 82, 0.6);
}

.gold .bar span {
  background: #f2c860;
  box-shadow: 0 0 10px rgba(242, 200, 96, 0.6);
}

.coral .bar span {
  width: 100% !important;
  background: #e65a6e;
}

.pct {
  flex: none;
  min-width: 40px;
  color: #7ee0b4;
  text-align: right;
}

.amber .pct {
  color: #f5b452;
}

.gold .pct {
  color: #f2c860;
}

.buttons {
  display: flex;
  flex: none;
  gap: 6px;
}

.buttons button {
  display: grid;
  place-items: center;
  width: 40px;
  height: 40px;
  border: 1px solid rgba(168, 140, 255, 0.45);
  border-radius: 50%;
  background: rgba(20, 16, 50, 0.7);
  color: var(--text);
  cursor: pointer;
}

.buttons button.text {
  width: auto;
  padding: 0 14px;
  border-radius: 999px;
}

.buttons svg {
  width: 18px;
  height: 18px;
  fill: none;
  stroke: currentColor;
  stroke-width: 2;
  stroke-linecap: round;
  stroke-linejoin: round;
}

.chooser {
  position: absolute;
  right: 18px;
  bottom: 44px;
  display: flex;
  flex-direction: column;
  gap: 12px;
  width: 340px;
  padding: 16px;
  border: 1px solid rgba(168, 140, 255, 0.45);
  border-radius: 14px;
  background: rgba(24, 18, 60, 0.97);
  box-shadow: 0 14px 40px rgba(0, 0, 0, 0.5);
}

.chooser header {
  display: flex;
  gap: 8px;
  align-items: center;
}

.chooser header b {
  flex: 1;
  font-weight: 500;
}

.chooser .title {
  margin: 0;
  overflow: hidden;
  white-space: nowrap;
  text-overflow: ellipsis;
  color: var(--text-dim, #b9b0d8);
  font-size: 13px;
}

.choices {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}

.choices button {
  padding: 7px 14px;
  border: 1px solid rgba(168, 140, 255, 0.4);
  border-radius: 999px;
  background: rgba(40, 30, 90, 0.5);
  color: var(--text);
  font: inherit;
  cursor: pointer;
}

.choices button.on {
  border-color: transparent;
  background: linear-gradient(135deg, #8a6cf0, #a98bff);
}

.start {
  padding: 11px;
  border: none;
  border-radius: 10px;
  background: linear-gradient(135deg, #7a5ce6, #a98bff);
  color: #fff;
  font: inherit;
  font-size: 15px;
  cursor: pointer;
}

.terms {
  color: var(--text-dim, #b9b0d8);
  font-size: 11px;
  text-align: center;
}
</style>
