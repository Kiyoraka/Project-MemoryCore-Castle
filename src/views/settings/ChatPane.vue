<script setup>
// Settings > Chat: the mic shortcut, the CHAT room, the companion in the room (emotion, public card), pictures, voice, and the life it shares.
// Cut from SettingsView.vue (Shard): the page's shared state comes through SETTINGS_PAGE.
import { computed, inject, onMounted, ref } from 'vue'
import GlassSelect from '../../components/GlassSelect.vue'
import SettingGroup from '../../components/settings/SettingGroup.vue'
import SettingRow from '../../components/settings/SettingRow.vue'
import SettingSwitch from '../../components/settings/SettingSwitch.vue'
import SettingChoice from '../../components/settings/SettingChoice.vue'
import SettingText from '../../components/settings/SettingText.vue'
import SettingPill from '../../components/settings/SettingPill.vue'
import { valueOf } from '../../components/settings/settingsDefaults.js'
import { isMac, shortcutLabel } from '../../platform.js'
import { DEFAULT_MIC_SHORTCUT, micShortcutOf, micShortcutText, readMicShortcut, sameMicShortcut } from '../../components/voicehold.js'
import { roomCard, roomCardSave, roomPicturesStatus, roomVoicesStatus, roomShelf, roomShelfRefresh, roomShelfRemove } from '../../api/castle.js'
import { SETTINGS_PAGE } from './settingsPage.js'

const { settings, keys, locked, saveRow, save, rows, companionLabel, words, go } = inject(SETTINGS_PAGE)

// ---------- Chat: the room's alerts and the companion in the room ----------

// the companion is in the room when CHAT made its room key (the key itself is never shown here)
const companionInRoom = computed(() => keys.value.some((k) => k.name === 'room-companion' && k.set))
const roomLocked = computed(() => !companionInRoom.value)
const card = ref({ open: false, text: '', saved: '' })
const PER_DAY = [0, 1, 2, 3, 4, 5].map((n) => ({ value: n, label: String(n) }))

// Pictures in the room: how the companion can draw, and today's numbers
const pictures = ref(null)
async function loadPictures() {
  try {
    pictures.value = await roomPicturesStatus()
  } catch {
    pictures.value = null
  }
}
async function savePictures(row, patch) {
  await saveRow(row, patch)
  await loadPictures()
}
const drawsWith = computed(() => {
  const way = pictures.value?.way
  if (way === 'codex') return 'Draws with Codex, on your ChatGPT subscription, whichever CLI your companion thinks with.'
  if (way === 'openai') return 'Draws with your ChatGPT API key, billed per picture.'
  if (way === 'none') return 'Needs Codex signed in with ChatGPT on this computer, or a ChatGPT key in Keys.'
  return ''
})
const canDraw = computed(() => !roomLocked.value && pictures.value?.way !== 'none')
const drawing = computed(() => canDraw.value && valueOf(settings.value, 'roomPicturesOn'))

// Voice in the room: whether an ElevenLabs key is saved, and today's numbers
const voices = ref(null)
async function loadVoices() {
  try {
    voices.value = await roomVoicesStatus()
  } catch {
    voices.value = null
  }
}
async function saveVoices(row, patch) {
  await saveRow(row, patch)
  await loadVoices()
}
const canSpeak = computed(() => !roomLocked.value && !(voices.value && !voices.value.hasKey))
const speaking = computed(() => canSpeak.value && valueOf(settings.value, 'roomVoicesOn'))

// The companion's life in the room: the shelf of notes it chose from its memory
const shelf = ref(null)
const shelfBusy = ref(false)
async function loadShelf() {
  try {
    shelf.value = await roomShelf()
  } catch {
    shelf.value = null
  }
}
async function remakeShelf() {
  shelfBusy.value = true
  rows.clear('shelf-notes')
  try {
    shelf.value = await roomShelfRefresh()
  } catch (reason) {
    rows.failed('shelf-notes', reason, 'Not read')
  } finally {
    shelfBusy.value = false
  }
}
async function switchShelf(on) {
  const saved = await saveRow('shelf', { roomShelfOn: on })
  if (saved && on && !shelf.value?.notes?.length) await remakeShelf()
}
async function takeOff(index) {
  rows.clear('shelf-notes')
  try {
    shelf.value = await roomShelfRemove(index)
  } catch (reason) {
    rows.failed('shelf-notes', reason, 'Not taken off')
  }
}
const shelfMade = computed(() => {
  const at = shelf.value?.madeAt
  if (!at) return `${companionLabel.value} has not read its memory for the room yet.`
  const count = shelf.value.notes?.length ?? 0
  const when = new Date(at * 1000).toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' })
  if (!count) return `Read on ${when}: nothing chosen to share this time.`
  return `Read on ${when}: ${count} ${count === 1 ? 'note' : 'notes'} chosen to share.`
})
function loadChat() {
  loadPictures()
  loadVoices()
  loadShelf()
}

async function openCard() {
  try {
    card.value = { open: true, text: await roomCard(), saved: '' }
  } catch (reason) {
    rows.failed('card', reason, 'Not opened')
  }
}

async function saveCard() {
  try {
    await roomCardSave(card.value.text)
    card.value = { ...card.value, saved: `Saved - this is all ${companionLabel.value} may say about itself in the room.` }
  } catch (reason) {
    card.value = { ...card.value, saved: String(reason?.message ?? reason) }
  }
}

// ---------- Chat's mic shortcut (PR #69) ----------

// The keys that start and stop Chat's mic, shown as key caps. Change waits for the next keys (Esc
// leaves without a change); a good choice saves at once, a refused one says why and keeps waiting.
const micShortcut = computed(() => micShortcutOf(settings.value?.micShortcut))
const micKeyCaps = computed(() => (isMac ? [...shortcutLabel(micShortcutText(micShortcut.value), true)] : micShortcutText(micShortcut.value).split('+')))
const micIsDefault = computed(() => sameMicShortcut(micShortcut.value, DEFAULT_MIC_SHORTCUT))
const micDefaultLabel = shortcutLabel(micShortcutText(DEFAULT_MIC_SHORTCUT))
const micRecording = ref(false)
const micNote = ref({ text: '', bad: false })
const micBox = ref(null)

function micChange() {
  if (micRecording.value) return micLeave()
  micRecording.value = true
  micNote.value = { text: '', bad: false }
  micBox.value?.focus()
}
function micLeave(text = `Nothing changed. Still ${shortcutLabel(micShortcutText(micShortcut.value))}.`) {
  if (!micRecording.value) return
  micRecording.value = false
  micNote.value = { text, bad: false }
}
async function micKeyDown(event) {
  if (!micRecording.value) {
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault()
      micChange()
    }
    return
  }
  event.preventDefault()
  event.stopPropagation()
  const read = readMicShortcut(event, isMac, (text) => shortcutLabel(text))
  if (!read) return
  if (read.cancel) return micLeave()
  if (read.problem) {
    micNote.value = { text: read.problem, bad: true }
    return
  }
  micRecording.value = false
  const saved = await save({ micShortcut: read.shortcut }, '')
  micNote.value = saved ? { text: `Saved: ${read.keys}. The mic in Chat answers to it now.`, bad: false } : { text: 'That did not save. Try again.', bad: true }
}
async function micReset() {
  micRecording.value = false
  const saved = await save({ micShortcut: null }, '')
  if (saved) micNote.value = { text: `Back to ${micDefaultLabel}.`, bad: false }
}

onMounted(async () => {
  loadChat()
})
</script>

<template>
  <SettingGroup id="mic" title="Mic" tip="The mic beside Send in the Chat tab of HOME, STORY and DEV. Its keys work anywhere in that Chat, the message box too. Esc cancels while it listens.">
    <SettingRow id="mic-keys" label="Start and stop the mic" :description="`${isMac ? '⌘' : 'Ctrl'} plus a letter or a digit, with ${isMac ? '⇧ or ⌥' : 'Shift or Alt'} if you like. Keys that copy, paste, undo or belong to ${isMac ? 'macOS' : 'Windows'} are refused.`">
      <span
        ref="micBox"
        class="key-caps"
        :class="{ recording: micRecording }"
        tabindex="0"
        role="button"
        data-takes-keys
        :aria-label="micRecording ? 'Press the new keys, or Esc to leave' : `Mic shortcut ${micKeyCaps.join(' ')}. Press Enter to change it.`"
        @click="micRecording || micChange()"
        @keydown="micKeyDown"
        @blur="micLeave()"
      >
        <span v-if="micRecording" class="key-wait">Press the new keys… Esc to leave</span>
        <kbd v-for="(cap, i) in micKeyCaps" v-else :key="i">{{ cap }}</kbd>
      </span>
      <button :disabled="locked" @mousedown.prevent @click="micChange">{{ micRecording ? 'Leave' : 'Change' }}</button>
      <button :disabled="locked || micIsDefault" @click="micReset">Reset to {{ micDefaultLabel }}</button>
      <template v-if="micNote.text" #below>
        <p class="hint" :class="{ bad: micNote.bad }" aria-live="polite">{{ micNote.text }}</p>
      </template>
    </SettingRow>
  </SettingGroup>

  <SettingGroup id="room" title="The room">
    <SettingRow id="chat-room" label="CHAT room" :description="settings.rooms?.chat === false ? 'Switched off in Rooms. These settings wait for it.' : 'On. Its tab is in the top bar.'">
      <SettingPill :tone="settings.rooms?.chat === false ? 'warn' : 'good'">{{ settings.rooms?.chat === false ? 'Off' : 'On' }}</SettingPill>
      <button @click="go('rooms#room-chat')">Open Rooms</button>
    </SettingRow>
    <SettingRow id="chat-alerts" label="Alerts for new messages" description="Notifications and sounds are set on the Alerts page.">
      <button @click="go('alerts#alert-chat')">Open Alerts</button>
    </SettingRow>
  </SettingGroup>

  <SettingGroup id="in-room" :title="`${companionLabel} in the room`" :note="roomLocked ? `Let ${companionLabel} in from the CHAT room's side panel to set these.` : ''">
    <SettingRow id="emotion" label="Emotion engine in the room" :disabled="roomLocked" :description="`Colours how ${companionLabel} sounds there, never what it knows.`" tip="Only how it sounds, never what it knows: its traits' one-line manner, and the ones meant for you only when it answers you.">
      <SettingSwitch :model-value="valueOf(settings, 'emotionRoomVoice')" label="Emotion engine in the room" :disabled="locked || roomLocked" @update:model-value="(on) => saveRow('emotion', { emotionRoomVoice: on })" />
    </SettingRow>
    <SettingRow id="card" label="Public card" :disabled="roomLocked" :description="`All ${companionLabel} may say about itself. Nothing from your memory.`">
      <button :disabled="roomLocked" @click="card.open ? (card.open = false) : openCard()">{{ card.open ? 'Close' : 'Edit' }}</button>
      <template v-if="card.open" #below>
        <div class="card-edit">
          <textarea v-model="card.text" rows="6" maxlength="2000" aria-label="Public card"></textarea>
          <div class="row"><button class="primary" @click="saveCard">Save the card</button><span v-if="card.saved" class="hint">{{ card.saved }}</span></div>
        </div>
      </template>
    </SettingRow>
  </SettingGroup>

  <SettingGroup id="pictures" title="Pictures" :tip="`${companionLabel} draws only when someone in the room asks for a picture, and never a real person, anything adult, violent or hateful, words in the picture, or anything private. Kiyo's daily cap for the room still applies on top of yours.`">
    <SettingRow id="pictures-on" :label="`Let ${companionLabel} draw`" :disabled="!canDraw">
      <template #description>
        <template v-if="roomLocked">Needs {{ companionLabel }} in the room first.</template>
        <template v-else-if="pictures?.way === 'none'">{{ drawsWith }} <a href="#" @click.prevent="go('keys#key-openai')">Add one in Keys</a></template>
        <template v-else>{{ drawsWith }}</template>
      </template>
      <SettingSwitch :model-value="valueOf(settings, 'roomPicturesOn')" :label="`Let ${companionLabel} draw`" :disabled="locked || !canDraw" @update:model-value="(on) => savePictures('pictures-on', { roomPicturesOn: on })" />
    </SettingRow>
    <SettingRow id="pictures-per-day" label="Pictures a day" :disabled="!drawing" description="Your own limit. The room keeps its own cap too." tip="Your own limit, 0 to 5 a day. The room keeps its own count too and refuses past Kiyo's cap, whichever comes first.">
      <GlassSelect class="per-day" :model-value="valueOf(settings, 'roomPicturesPerDay')" :options="PER_DAY" :disabled="locked || !drawing" aria-label="Pictures a day" @update:model-value="(n) => savePictures('pictures-per-day', { roomPicturesPerDay: Number(n) })" />
    </SettingRow>
    <SettingRow
      id="pictures-quality"
      label="Quality"
      :disabled="!drawing || pictures?.way !== 'openai'"
      :description="pictures?.way === 'openai' ? 'About $0.02 a picture at Low, $0.04 at Medium.' : 'Only when drawing with a ChatGPT key.'"
      tip="Each picture spends your ChatGPT key: about $0.02 at Low and $0.04 at Medium. Codex draws with your subscription instead."
    >
      <SettingChoice :model-value="valueOf(settings, 'roomPictureQuality') === 'medium' ? 'medium' : 'low'" :options="[{ value: 'low', label: 'Low' }, { value: 'medium', label: 'Medium' }]" label="Quality" :disabled="locked || !drawing || pictures?.way !== 'openai'" @update:model-value="(q) => savePictures('pictures-quality', { roomPictureQuality: q })" />
    </SettingRow>
    <SettingRow v-if="pictures && drawing" id="pictures-today" label="Today">
      <template #description>{{ pictures.usedToday }} of {{ pictures.limit }} used.<template v-if="pictures.roomCap != null"> The room allows {{ pictures.roomCap }} ({{ pictures.roomToday ?? 0 }} used).</template></template>
    </SettingRow>
  </SettingGroup>

  <SettingGroup id="voice" title="Voice" :tip="`${companionLabel} speaks a short line aloud when someone asks to hear it, or when a line is warmer spoken. Everyone in the room hears it and sees the words. If a voice cannot be made, the words are posted as text instead. Kiyo's daily cap for the room still applies on top of yours.`">
    <SettingRow id="voices-on" :label="`Let ${companionLabel} speak`" :disabled="!canSpeak">
      <template #description>
        <template v-if="roomLocked">Needs {{ companionLabel }} in the room first.</template>
        <template v-else-if="voices && !voices.hasKey">Needs an ElevenLabs key. <a href="#" @click.prevent="go('keys#key-elevenlabs')">Add one in Keys</a></template>
        <template v-else>Speaks with your ElevenLabs key, billed by the characters spoken.</template>
      </template>
      <SettingSwitch :model-value="valueOf(settings, 'roomVoicesOn')" :label="`Let ${companionLabel} speak`" :disabled="locked || !canSpeak" @update:model-value="(on) => saveVoices('voices-on', { roomVoicesOn: on })" />
    </SettingRow>
    <SettingRow id="voices-per-day" label="Voices a day" :disabled="!speaking" description="Your own limit. The room keeps its own cap too." tip="Your own limit, 0 to 5 a day. The room keeps its own count too and refuses past Kiyo's cap, whichever comes first.">
      <GlassSelect class="per-day" :model-value="valueOf(settings, 'roomVoicesPerDay')" :options="PER_DAY" :disabled="locked || !speaking" aria-label="Voices a day" @update:model-value="(n) => saveVoices('voices-per-day', { roomVoicesPerDay: Number(n) })" />
    </SettingRow>
    <SettingRow id="voice-id" label="ElevenLabs voice ID" :disabled="!speaking" description="Empty for the default voice, the same one STUDIO uses." tip="Copy a voice's ID from your ElevenLabs Voices page. Leave it empty for the default voice, the same one STUDIO uses.">
      <SettingText :model-value="settings.roomVoiceId || ''" label="ElevenLabs voice ID" placeholder="the default voice" :maxlength="64" :disabled="locked || !speaking" @save="(v) => saveVoices('voice-id', { roomVoiceId: v.trim() || null })" />
    </SettingRow>
    <SettingRow v-if="voices && speaking" id="voices-today" label="Today">
      <template #description>{{ voices.usedToday }} of {{ voices.limit }} spoken.<template v-if="voices.roomCap != null"> The room allows {{ voices.roomCap }} ({{ voices.roomToday ?? 0 }} used).</template></template>
    </SettingRow>
  </SettingGroup>

  <SettingGroup id="life" title="Your life in the room" :tip="`${companionLabel} reads its memory in private - your recent diary, what it is up to, today's look, your projects, your profile and its traits - and chooses what it would like to share. The room only ever gets those notes, never the memory itself, so no one there can ask it for more. You can take any note off.`">
    <SettingRow id="shelf" :label="`Let ${companionLabel} share your life`" :disabled="roomLocked" :description="roomLocked ? `Needs ${companionLabel} in the room first.` : 'Only the notes it chooses, never the memory itself.'">
      <SettingSwitch :model-value="valueOf(settings, 'roomShelfOn')" :label="`Let ${companionLabel} share your life`" :disabled="locked || roomLocked" @update:model-value="switchShelf" />
    </SettingRow>
    <SettingRow id="shelf-notes" label="Shared notes" :disabled="roomLocked || !settings.roomShelfOn" :description="settings.roomShelfOn ? shelfMade : 'Turn on sharing to see them.'" tip="It reads again by itself when your memory changes, at most every 3 hours.">
      <button :disabled="locked || shelfBusy || shelf?.making || roomLocked || !settings.roomShelfOn" @click="remakeShelf">{{ shelfBusy || shelf?.making ? 'Reading…' : 'Read my memory again' }}</button>
      <template v-if="settings.roomShelfOn && shelf?.notes?.length" #below>
        <ul class="shelf">
          <li v-for="(note, i) in shelf.notes" :key="i + note">
            <span>{{ note }}</span>
            <button class="take-off" :disabled="locked || shelfBusy" :aria-label="'Take this note off: ' + note" title="Take it off" @click="takeOff(i)">&#x2715;</button>
          </li>
        </ul>
      </template>
    </SettingRow>
  </SettingGroup>
</template>

<style scoped src="./settings-page.css"></style>
