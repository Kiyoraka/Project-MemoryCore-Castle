<script setup>
import { onBeforeUnmount, onMounted, ref } from 'vue'
import MarkdownText from './MarkdownText.vue'
import { chooseLine, isDesignGroup } from './designpick.js'

// Chat's conversation: the member's bubbles right, the companion's left, plan cards, pictures side
// by side, voice lines, the typing bubble, and the card for a choice asked in the Log. Only shows;
// every action goes up to ChatView.
defineProps({
  // "asleep", "waiting", "ok" or "unreadable"
  status: { type: String, default: 'asleep' },
  items: { type: Array, default: () => [] },
  typing: { type: Boolean, default: false },
  // path -> data URL, and voice lengths (useChatMedia)
  media: { type: Object, required: true },
  durations: { type: Object, required: true },
  playing: { type: String, default: null },
  // a choice menu read from the Log screen, or null
  menu: { type: Object, default: null },
  // Codex is asking for an approval on the Log screen
  codexAsking: { type: Boolean, default: false },
  companion: { type: String, default: 'Your companion' },
  cli: { type: String, default: 'claude' },
})
const emit = defineEmits(['show-log', 'send-now', 'open-plan', 'open-picture', 'toggle-voice', 'play-voice', 'choose', 'choose-designs'])

// Designs to choose from (two to four pictures from one Design folder): a tick on each, one Choose.
const designTicks = ref({}) // group id -> [paths ticked]
const designsSent = ref({}) // group id -> the line sent
function tickDesign(group, path, on) {
  const now = designTicks.value[group] ?? []
  designTicks.value = { ...designTicks.value, [group]: on ? [...now, path] : now.filter((p) => p !== path) }
}
function chooseDesigns(item) {
  const line = chooseLine(item.list, designTicks.value[item.id] ?? [])
  if (!line) return
  emit('choose-designs', line)
  designsSent.value = { ...designsSent.value, [item.id]: line }
  designTicks.value = { ...designTicks.value, [item.id]: [] }
}

const TICK_TITLES = {
  sending: 'Sent to the terminal',
  queued: 'Waiting in the queue - read after the current step',
  read: 'Read',
}

const fileName = (path) => path.split(/[\\/]/).pop()

function clock(at) {
  if (!at) return ''
  const when = new Date(at)
  return Number.isNaN(when.getTime()) ? '' : when.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' })
}

function length(seconds) {
  if (!seconds || !Number.isFinite(seconds)) return ''
  const s = Math.round(seconds)
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, '0')}`
}

const scroller = ref(null)
const nearBottom = () => {
  const el = scroller.value
  return !el || el.scrollHeight - el.scrollTop - el.clientHeight < 80
}
const toBottom = () => {
  if (scroller.value) scroller.value.scrollTop = scroller.value.scrollHeight
}

// The chat starts at its newest message and stays there until the member scrolls up. It sits in a
// tab that is hidden (display: none) while it loads, where scrolling does nothing, so the newest
// message is also found again when the tab is shown, when a picture finishes loading, or when
// lines are added - as long as the member has not scrolled away from the bottom.
let stick = true
let watching = null
let following = null
const onScroll = () => {
  const el = scroller.value
  if (el && el.clientHeight > 0) stick = nearBottom()
}
const follow = () => {
  if (stick) toBottom()
}
onMounted(() => {
  const el = scroller.value
  if (!el) return
  el.addEventListener('scroll', onScroll, { passive: true })
  el.addEventListener('load', follow, true) // pictures, which grow the page after the lines
  if (typeof ResizeObserver !== 'undefined') {
    watching = new ResizeObserver(follow) // hidden -> shown, or the window resized
    watching.observe(el)
  }
  if (typeof MutationObserver !== 'undefined') {
    following = new MutationObserver(follow)
    following.observe(el, { childList: true, subtree: true })
  }
  toBottom()
})
onBeforeUnmount(() => {
  scroller.value?.removeEventListener('scroll', onScroll)
  scroller.value?.removeEventListener('load', follow, true)
  watching?.disconnect()
  following?.disconnect()
})

defineExpose({ nearBottom, toBottom })
</script>

<template>
  <div ref="scroller" class="messages" aria-live="polite">
    <p v-if="status === 'waiting'" class="quiet">The chat appears as soon as {{ companion }} starts talking.</p>
    <p v-else-if="status === 'unreadable'" class="quiet">
      Chat cannot read this session.
      <button class="link" @click="emit('show-log')">Use Log</button>
    </p>

    <template v-for="item in items" :key="item.id">
      <div v-if="item.kind === 'message'" class="row" :class="item.entry.role">
        <div class="bubble">
          <MarkdownText :text="item.entry.text" size="chat" />
          <div v-if="item.entry.pictures?.length" class="shared">
            <button
              v-for="path in item.entry.pictures"
              :key="path"
              class="shared-thumb"
              :title="fileName(path)"
              @click="emit('open-picture', path)"
            >
              <img v-if="media[path]" :src="media[path]" :alt="fileName(path)" />
              <span v-else class="thumb-wait">{{ media[path] === null ? fileName(path) : 'Loading…' }}</span>
            </button>
          </div>
          <span class="time">
            <button v-if="item.entry.state === 'queued' && cli === 'claude'" class="send-now" title="Send it now (Ctrl+X Ctrl+S)" @click="emit('send-now')">send now</button>
            {{ clock(item.entry.at) }}
            <span v-if="item.entry.role === 'member' && item.entry.state" class="ticks" :class="item.entry.state" :title="TICK_TITLES[item.entry.state]">{{ item.entry.state === 'read' ? '✓✓' : '✓' }}</span>
          </span>
        </div>
      </div>

      <div v-else-if="item.kind === 'plan'" class="row companion">
        <button class="plan" :title="item.title" @click="emit('open-plan', item.entry.text)">
          <span class="plan-tag">PLAN</span>
          <span class="plan-title">{{ item.title }}</span>
          <span class="plan-meta">{{ item.phases ? `${item.phases} ${item.phases === 1 ? 'phase' : 'phases'} - ` : '' }}tap to read<template v-if="clock(item.entry.at)"> · {{ clock(item.entry.at) }}</template></span>
        </button>
      </div>

      <div v-else-if="item.kind === 'pictures'" class="row companion">
        <div class="pictures">
          <div v-for="picture in item.list" :key="picture.id" class="thumb-wrap">
            <button class="thumb" :title="picture.text" @click="emit('open-picture', picture.path)">
              <img v-if="media[picture.path]" :src="media[picture.path]" :alt="picture.text" />
              <span v-else class="thumb-wait">{{ media[picture.path] === null ? 'Not available' : 'Loading…' }}</span>
            </button>
            <label v-if="isDesignGroup(item.list)" class="tick"><input type="checkbox" :checked="(designTicks[item.id] ?? []).includes(picture.path)" @change="tickDesign(item.id, picture.path, $event.target.checked)" /> {{ fileName(picture.path) }}</label>
          </div>
          <div v-if="isDesignGroup(item.list)" class="design-choose">
            <button class="choose" :disabled="!(designTicks[item.id] ?? []).length" @click="chooseDesigns(item)">{{ (designTicks[item.id] ?? []).length > 1 ? `Choose these ${(designTicks[item.id] ?? []).length}` : 'Choose' }}</button>
            <span v-if="designsSent[item.id]" class="sent">Sent: {{ designsSent[item.id].split(' - ')[0] }}</span>
          </div>
        </div>
      </div>

      <div v-else-if="item.kind === 'voice'" class="row companion">
        <div class="voice" :class="{ playing: playing === item.entry.path }">
          <button class="play" :aria-label="playing === item.entry.path ? 'Pause' : 'Play'" @click="emit('toggle-voice', item.entry)">
            {{ playing === item.entry.path ? '❚❚' : '▶' }}
          </button>
          <span class="voice-label">{{ companion }}'s voice</span>
          <span class="voice-length">{{ length(durations[item.entry.path]) }}</span>
          <button class="again" aria-label="Play again" title="Play again" @click="emit('play-voice', item.entry)">↻</button>
        </div>
      </div>

    </template>

    <div v-if="codexAsking" class="codex-ask" role="status">
      {{ companion }} is asking for your approval.
      <button class="link" @click="emit('show-log')">Answer in Log</button>
    </div>

    <div v-if="typing" class="row companion" :aria-label="`${companion} is typing`">
      <div class="typing" role="status"><span></span><span></span><span></span></div>
    </div>
  </div>

  <div v-if="menu" class="choices" role="group" :aria-label="menu.question || `${companion} is asking`">
    <p class="ask">{{ menu.question || `${companion} is asking` }}</p>
    <div class="options">
      <button v-for="option in menu.options" :key="option.number" class="option" :title="option.hint" @click="emit('choose', option.number)">
        <span class="number">{{ option.number }}</span>{{ option.label }}
      </button>
      <button class="option log" @click="emit('show-log')">Open Log</button>
    </div>
  </div>
</template>

<style scoped>
.messages {
  display: flex;
  flex-direction: column;
  gap: 8px;
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  padding: 4px 4px 8px 0;
  /* no grey bar; the wheel and touch still scroll */
  scrollbar-width: none;
}

.row {
  display: flex;
}

.row.member {
  justify-content: flex-end;
}

.bubble {
  position: relative;
  max-width: 82%;
  padding: 8px 12px 18px;
  border-radius: 14px;
  font-size: 14px;
  line-height: 1.45;
  overflow-wrap: anywhere;
}

.companion .bubble {
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid var(--line);
  border-top-left-radius: 4px;
}

.member .bubble {
  background: color-mix(in srgb, var(--accent) 34%, transparent);
  border-top-right-radius: 4px;
}

.time {
  position: absolute;
  right: 10px;
  bottom: 4px;
  font-size: 10px;
  color: var(--text-dim);
}

/* The member's ticks: one grey while it travels, one while it waits in the queue, two blue once read. */
.ticks {
  margin-left: 3px;
  font-size: 11px;
  letter-spacing: -3px;
}

.ticks.sending {
  opacity: 0.45;
}

.ticks.read {
  color: #7fd8ff;
}

.send-now {
  margin-right: 6px;
  padding: 0 6px;
  border: 1px solid currentColor;
  border-radius: 8px;
  background: none;
  color: inherit;
  font: inherit;
  font-size: 10px;
  cursor: pointer;
}

/* room for the time and the ticks (and "send now") under a short message */
.member .bubble {
  min-width: 96px;
}

.member .bubble:has(.send-now) {
  min-width: 170px;
}

.send-now:hover {
  color: var(--text);
}

/* Three dots rising in turn, as a messenger shows someone typing. */
.typing {
  display: inline-flex;
  align-items: center;
  gap: 5px;
  padding: 12px 16px;
  border: 1px solid var(--line);
  border-radius: 14px;
  border-top-left-radius: 4px;
  background: rgba(255, 255, 255, 0.08);
}

.typing span {
  width: 7px;
  height: 7px;
  border-radius: 50%;
  background: var(--text-dim);
  animation: typing 1.2s infinite ease-in-out;
}

.typing span:nth-child(2) {
  animation-delay: 0.15s;
}

.typing span:nth-child(3) {
  animation-delay: 0.3s;
}

@keyframes typing {
  0%,
  60%,
  100% {
    transform: translateY(0);
    opacity: 0.45;
  }
  30% {
    transform: translateY(-5px);
    opacity: 1;
  }
}

@media (prefers-reduced-motion: reduce) {
  .typing span {
    animation: none;
  }
}

.quiet {
  margin: auto;
  font-size: 13px;
  color: var(--text-dim);
  text-align: center;
}

button {
  font: inherit;
  cursor: pointer;
}

.link {
  padding: 0;
  border: 0;
  background: none;
  color: var(--accent);
  font-size: 12px;
  text-decoration: underline;
}


.pictures {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  max-width: 82%;
}

.thumb-wrap {
  display: flex;
  flex-direction: column;
  gap: 4px;
}

.tick {
  display: flex;
  align-items: center;
  gap: 6px;
  max-width: 180px;
  font-size: 11.5px;
  color: var(--text-dim);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.design-choose {
  display: flex;
  flex-basis: 100%;
  align-items: center;
  gap: 10px;
}

.design-choose .choose {
  padding: 6px 14px;
  border: 1px solid var(--accent);
  border-radius: 8px;
  background: var(--accent);
  color: #07142e;
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.design-choose .choose:disabled {
  opacity: 0.45;
  cursor: default;
}

.design-choose .sent {
  font-size: 12px;
  color: var(--text-dim);
}

.thumb {
  display: grid;
  place-items: center;
  width: 180px;
  height: 120px;
  padding: 0;
  overflow: hidden;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: rgba(0, 0, 0, 0.25);
  cursor: zoom-in;
}

.thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.thumb-wait {
  font-size: 11px;
  color: var(--text-dim);
}

.plan {
  display: flex;
  flex-direction: column;
  gap: 3px;
  width: min(320px, 82%);
  padding: 10px 14px;
  border: 1px solid var(--line);
  border-left: 3px solid var(--accent);
  border-radius: 4px 14px 14px 14px;
  background: rgba(255, 255, 255, 0.08);
  color: var(--text);
  text-align: left;
}

.plan:hover {
  background: var(--hover);
}

.plan-tag {
  font-size: 10px;
  font-weight: 700;
  letter-spacing: 0.18em;
  color: var(--accent);
}

.plan-title {
  font-size: 14px;
  font-weight: 600;
  line-height: 1.35;
  overflow-wrap: anywhere;
}

.plan-meta {
  font-size: 11px;
  color: var(--text-dim);
}

.voice {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 12px;
  border: 1px solid var(--line);
  border-radius: 20px;
  background: rgba(255, 255, 255, 0.08);
}

.row.companion:has(.voice-words) {
  flex-direction: column;
  align-items: flex-start;
}

.voice-words {
  max-width: 82%;
  margin-top: 6px;
  padding: 8px 12px;
  border: 1px solid var(--line);
  border-radius: 4px 14px 14px 14px;
  background: rgba(255, 255, 255, 0.06);
  font-size: 13.5px;
  line-height: 1.5;
}

.voice-words p {
  margin: 0;
  white-space: pre-wrap;
}

.voice-words .english {
  margin-top: 6px;
  color: var(--text-dim);
  font-style: italic;
}

.voice.playing {
  border-color: var(--accent);
}

.voice .play,
.voice .again {
  display: grid;
  place-items: center;
  width: 30px;
  height: 30px;
  padding: 0;
  border: 0;
  border-radius: 50%;
  font-size: 12px;
}

.voice .play {
  background: var(--accent);
  color: #160f33;
}

.voice .again {
  background: none;
  color: var(--text-dim);
  font-size: 16px;
}

.voice-label {
  font-size: 13px;
}

.voice-length {
  min-width: 32px;
  font-size: 12px;
  color: var(--text-dim);
}

.choices {
  display: flex;
  flex-direction: column;
  gap: 6px;
  margin-bottom: 8px;
  padding: 10px 12px;
  border: 1px solid var(--accent);
  border-radius: 12px;
  background: color-mix(in srgb, var(--accent) 12%, transparent);
}

.ask {
  margin: 0;
  font-size: 13px;
  font-weight: 600;
}

.options {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
}

.option {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  max-width: 100%;
  padding: 6px 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.2);
  color: var(--text);
  font-size: 12.5px;
  text-align: left;
}

.option:hover {
  background: var(--hover);
}

.option .number {
  display: grid;
  place-items: center;
  flex: none;
  width: 18px;
  height: 18px;
  border-radius: 50%;
  background: var(--accent);
  color: #160f33;
  font-size: 11px;
  font-weight: 700;
}

.option.log {
  color: var(--text-dim);
}

.shared {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  margin-top: 8px;
}

.shared-thumb {
  display: grid;
  place-items: center;
  padding: 0;
  overflow: hidden;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.25);
  cursor: zoom-in;
}

.shared-thumb {
  width: 150px;
  height: 100px;
}

.shared-thumb img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}


button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

.codex-ask {
  align-self: center;
  margin: 8px 0;
  padding: 8px 14px;
  border: 1px solid var(--warn);
  border-radius: 10px;
  color: var(--text);
  font-size: 13px;
}
</style>
