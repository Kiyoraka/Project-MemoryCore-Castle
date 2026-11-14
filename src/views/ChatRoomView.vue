<script setup>
// CHAT: the MemoryCore Foundation room - the member, the room's hosts, other members and their
// companions, as a chat: every speaker's lines (read by Castle's room reader from the moment it
// starts), the member's own on the right with WhatsApp-like ticks, a face beside everyone else's.
// Until the member is in, the room shows its guide door. The companion takes its own turns in
// Castle's core (roomturn.rs: a room-only run with no tools) - nothing from the room ever reaches
// its private main session; LOG shows those turns.
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { rememberLibraryOpened, rememberedLibraryOpened, roomSteps } from '../components/roomsteps.js'
import { listen } from '@tauri-apps/api/event'
import { choosePicture, getSettings, keyStatus, memberSignIn, memberSignOut, memberStatus, openUrl, roomAvatar, roomBacklog, roomCard, roomCardSave, roomCompanionIn, roomFaceSet, roomFile, roomLog, roomMe, roomMine, roomPost, roomPresence, roomReset, roomSeen, roomVisible, saveSettings } from '../api/castle.js'
import { avatarLoader, currentAvatar, datedLines, initial, isMine, roomMembers, roomPresenceState, speakerPresentation, ticks } from '../components/roomchat.js'
import { roomNameSet } from '../api/castle.js'
import { avatarGet } from '../api/castle.js'

const props = defineProps({
  visible: { type: Boolean, default: true },
  companion: { type: String, default: 'Your companion' },
})
const emit = defineEmits(['fresh'])

const settings = ref(null)
const keys = ref([])
const me = ref(null)
const companionProfile = ref(null)
const companionPortrait = ref(null)
const lines = ref([])
const seen = ref([]) // who has read how far (the ticks)
const myIds = ref([]) // the lines Castle posted for the member
const faces = ref({}) // avatar token -> picture
const tab = ref('chat') // CHAT | LOG
const readProblem = ref('')
const faceLine = ref('')
const displayName = ref('')
const nameEditing = ref(false)
const nameSaving = ref(false)
const nameLine = ref('')
const turnLog = ref('') // the companion's room turns, raw
const card = ref({ open: false, text: '', saved: '' })
const presence = ref([])
const refused = ref(null)
const draft = ref('')
const files = ref({})
const scroller = ref(null)
// the member's door, step by step (member_status): signedIn, card, foundation, name, roomOpen, expired
const member = ref(null)
const signing = ref(false)
const roomSaving = ref(false)
const presenceFresh = ref(false) // the last presence read worked
const companionVerified = ref(false) // the room confirmed this Castle's companion identity
let presenceRead = 0 // an older presence read must not overwrite a newer one
const doorProblem = ref('')
const PORTFOLIO = 'https://www.afifmaahi-portfolio.com'
let alive = true

const haveCompanionKey = computed(() => keys.value.some((k) => k.name === 'room-companion' && k.set))
const ready = computed(() => !!member.value?.roomOpen)
const locked = computed(() => !ready.value || refused.value?.kind === 'locked' || refused.value?.kind === 'notSetUp')
const mode = computed(() => settings.value?.roomCompanionMode ?? 'off')
const today = ref(new Date())
const shown = computed(() => datedLines(lines.value, today.value))
const avatarToken = (author) => currentAvatar(author, presence.value)
const myPicture = computed(() => faces.value[avatarToken(me.value)] || null)
const companionPicture = computed(() => faces.value[avatarToken(companionProfile.value)] || companionPortrait.value)
const hostName = computed(() => {
  const present = [...new Set(presence.value.filter((p) => p.kind === 'host').map((p) => p.name))]
  const recorded = [...new Set(lines.value.filter((l) => l.author?.kind === 'host').map((l) => l.author.name))]
  const names = present.length ? present : recorded
  return names.length === 1 ? names[0] : ''
})
const speaker = (author) => speakerPresentation(author, hostName.value)

const isAi = (l) => ['assistant', 'member_assistant'].includes(l.author?.kind)
const mine = (l) => isMine(l, myIds.value, me.value)

function clock(at) {
  const d = new Date(at)
  return Number.isNaN(d.getTime()) ? '' : d.toLocaleTimeString('en-US', { hour: 'numeric', minute: '2-digit' })
}

async function loadFile(id) {
  if (!id || files.value[id] !== undefined) return
  files.value = { ...files.value, [id]: null }
  try {
    files.value = { ...files.value, [id]: await roomFile(id) }
  } catch {
    // shown as a name only
  }
}

const loadFace = avatarLoader(roomAvatar, (token, face) => {
  faces.value = { ...faces.value, [token]: face }
})

// Lines from the room reader (or a line just sent): kept once each, in order; cleared ones go.
function take(fresh, cleared = [], quiet = false) {
  const gone = new Set(cleared)
  const added = (fresh ?? []).filter((l) => !lines.value.some((x) => x.id === l.id))
  const merged = new Map(lines.value.map((l) => [l.id, l]))
  for (const l of fresh ?? []) merged.set(l.id, l)
  lines.value = [...merged.values()].filter((l) => !gone.has(l.id)).sort((x, y) => x.id - y.id).slice(-500)
  lines.value.forEach((l) => {
    loadFile(l.voice?.fileId)
    loadFile(l.picture?.fileId)
    loadFace(avatarToken(l.author))
  })
  if (added.length && !quiet) {
    readProblem.value = ''
    if (!props.visible) emit('fresh')
  }
  if (added.length && props.visible) scrollDown()
}

async function refreshPresence() {
  today.value = new Date()
  if (!ready.value || !props.visible) return
  const read = ++presenceRead
  try {
    const rows = roomMembers(await roomPresence())
    if (!alive || read !== presenceRead) return
    presence.value = rows
    presenceFresh.value = true
    presence.value.forEach((x) => loadFace(x.avatar))
    lines.value.forEach((l) => loadFace(avatarToken(l.author)))
    me.value = await roomMe()
    loadFace(avatarToken(me.value))
    if (haveCompanionKey.value) {
      try {
        const profile = await roomMe('companion')
        if (!alive || read !== presenceRead) return
        companionProfile.value = profile
        // keep the last answer while this one is on its way: the dot only goes grey when the check fails
        companionVerified.value = !!profile?.name
        loadFace(avatarToken(companionProfile.value))
      } catch {
        // Retain the last picture while the room is temporarily unavailable, but do not claim it is online.
        if (read === presenceRead) companionVerified.value = false
      }
    } else {
      companionProfile.value = null
      companionVerified.value = false
    }
    seen.value = await roomSeen()
  } catch {
    // the read loop reports refusals
    if (read === presenceRead) presenceFresh.value = false
  }
}

const presenceState = (row) => roomPresenceState(row, { me: me.value, companion: companionProfile.value, enabled: haveCompanionKey.value, verified: companionVerified.value, fresh: presenceFresh.value })

function scrollDown() {
  requestAnimationFrame(() => scroller.value && (scroller.value.scrollTop = scroller.value.scrollHeight))
}

async function send() {
  const text = draft.value.trim()
  if (!text) return
  try {
    const line = await roomPost(text)
    draft.value = ''
    if (line?.id) myIds.value = [...myIds.value, line.id]
    take([line], [], true)
    scrollDown()
  } catch (reason) {
    refused.value = reason
  }
}

// ---------- The companion's turns (taken in Castle's core) ----------

async function loadLog() {
  try {
    turnLog.value = await roomLog()
  } catch {
    // nothing yet
  }
}

async function openCard() {
  try {
    card.value = { open: true, text: await roomCard(), saved: '' }
  } catch (reason) {
    doorProblem.value = String(reason?.message ?? reason)
  }
}

async function saveCard() {
  try {
    await roomCardSave(card.value.text)
    card.value = { ...card.value, saved: 'Saved - this is all your companion may say about itself in the room.' }
  } catch (reason) {
    card.value = { ...card.value, saved: String(reason?.message ?? reason) }
  }
}

async function setMode(next) {
  try {
    settings.value = (await saveSettings({ ...settings.value, roomCompanionMode: next })).settings
  } catch (reason) {
    refused.value = { kind: 'other', message: String(reason) }
  }
}

// Her emotion engine colours how she talks here: only her traits' one-line manner, by audience.
async function setRoomVoice(on) {
  try {
    settings.value = (await saveSettings({ ...settings.value, emotionRoomVoice: on })).settings
  } catch (reason) {
    refused.value = { kind: 'other', message: String(reason) }
  }
}

// ---------- The door: a guide that opens by itself ----------

const STEP = { done: '✓', wait: '…', todo: '' }
const libraryOpened = ref(rememberedLibraryOpened())
const steps = computed(() => roomSteps(member.value, libraryOpened.value))

async function checkDoor() {
  try {
    const was = member.value?.roomOpen
    member.value = await memberStatus()
    if (member.value?.roomOpen && was === false) {
      emit('fresh') // the door just opened: the tab glows
      refused.value = null
      await roomReset().catch(() => {})
      lines.value = []
    }
  } catch {
    // outside Castle's window
  }
}

async function signIn() {
  doorProblem.value = ''
  signing.value = true
  try {
    member.value = await memberSignIn()
    refused.value = null
    await roomReset().catch(() => {})
  } catch (reason) {
    doorProblem.value = String(reason?.message ?? reason)
  } finally {
    signing.value = false
  }
}

async function signOut() {
  try {
    await roomCompanionIn(false).catch(() => {})
    await memberSignOut()
  } finally {
    member.value = null
    lines.value = []
    await roomReset().catch(() => {})
    await checkDoor()
    keys.value = await keyStatus().catch(() => keys.value)
  }
}

async function setCompanionIn(on) {
  if (roomSaving.value) return
  roomSaving.value = true
  ++presenceRead // a presence read that started before this change must not undo it
  doorProblem.value = ''
  try {
    await roomCompanionIn(on)
    keys.value = await keyStatus()
    await refreshPresence()
    if (!on && mode.value !== 'off') await setMode('off')
  } catch (reason) {
    doorProblem.value = String(reason?.message ?? reason)
  } finally { roomSaving.value = false }
}

async function changeParticipation(next) {
  if (roomSaving.value || !haveCompanionKey.value) return
  roomSaving.value = true
  try { await setMode(next) } finally { roomSaving.value = false }
}

const openLibrary = () => openUrl(`${PORTFOLIO}/alice/library`).catch(() => {})

async function saveDisplayName() {
  if (nameSaving.value) return
  nameSaving.value = true
  nameLine.value = ''
  const ownIds = lines.value.filter(mine).map((l) => l.id)
  let saved = false
  try {
    await roomNameSet(displayName.value.trim())
    saved = true
    // Retain ownership of existing lines even after the server's display name changes.
    myIds.value = [...new Set([...myIds.value, ...ownIds])]
    nameLine.value = 'Display name saved. Refreshing the room…'
    const updated = await roomMe()
    me.value = updated
    lines.value = lines.value.map((l) => ownIds.includes(l.id) ? { ...l, author: { ...l.author, name: updated.name } } : l)
    nameEditing.value = false
    nameLine.value = 'Your public display name is saved.'
    await checkDoor()
    await refreshPresence()
  } catch (reason) {
    nameLine.value = `${saved ? 'Name saved, but the room could not refresh: ' : ''}${String(reason?.message ?? reason)}`
  } finally { nameSaving.value = false }
}
// Sign in happens on the Library page; Castle then connects to that sign-in (the desktop page shows
// "Continue as <name>", one click).
function signInOnLibrary() {
  doorProblem.value = ''
  openLibrary()
  libraryOpened.value = true
  rememberLibraryOpened()
}

// The member's face in the room (their Google picture by default), and their companion's.
async function setFace(companion, reset = false) {
  faceLine.value = ''
  try {
    const path = reset ? null : await choosePicture(companion ? `A picture for ${props.companion}` : 'Your picture in the room')
    if (!reset && !path) return
    await roomFaceSet(path, companion)
    faceLine.value = reset ? 'Back to your Google picture.' : 'Your new picture is in the room.'
    refreshPresence()
  } catch (reason) {
    faceLine.value = String(reason?.message ?? reason)
  }
}

// While the door is shut, Castle looks again by itself: every 30 s while CHAT is shown, every
// five minutes while hidden.
let doorTimer = 0
function scheduleDoor() {
  clearTimeout(doorTimer)
  if (!alive) return
  doorTimer = setTimeout(async () => {
    if (!ready.value) await checkDoor()
    scheduleDoor()
  }, props.visible ? 30000 : 300000)
}

let presenceTimer
const stops = []
onMounted(async () => {
  try {
    settings.value = (await getSettings()).settings
    keys.value = await keyStatus()
    companionPortrait.value = await avatarGet().catch(() => null)
  } catch {
    // outside Castle's window
  }
  try {
    stops.push(await listen('room-lines', (e) => take(e.payload.lines, e.payload.cleared)))
    stops.push(await listen('room-turn', () => loadLog()))
    stops.push(
      await listen('room-read-failed', (e) => {
        const r = e.payload
        if (['locked', 'notSetUp', 'off'].includes(r.kind)) refused.value = r
        else readProblem.value = `The room could not be read (${r.message}) - trying again.`
      }),
    )
    const backlog = await roomBacklog()
    take(backlog.lines, [], true)
    if (backlog.problem && !['locked', 'notSetUp', 'off'].includes(backlog.problem.kind)) readProblem.value = `The room could not be read (${backlog.problem.message}) - trying again.`
    myIds.value = await roomMine()
    loadLog()
    await roomVisible(props.visible)
  } catch {
    // outside Castle's window
  }
  await checkDoor()
  scheduleDoor()
  presenceTimer = setInterval(refreshPresence, 10000)
  refreshPresence()
  scrollDown()
})
onBeforeUnmount(() => {
  alive = false
  clearInterval(presenceTimer)
  clearTimeout(doorTimer)
  stops.forEach((stop) => stop())
  roomVisible(false).catch(() => {})
})
watch(() => props.visible, (on) => {
  roomVisible(on).catch(() => {})
  if (on) {
    refreshPresence()
    scrollDown()
    if (!ready.value) checkDoor()
  }
  scheduleDoor()
})
</script>

<template>
  <div class="chat-room">
    <section v-if="locked" class="door widget">
      <p class="eyebrow">MEMORYCORE FOUNDATION</p>
      <h2>The room for MemoryCore Foundation members</h2>
      <p class="hint">Three steps, and the door opens by itself - nothing to paste.</p>
      <ol class="steps">
        <li v-for="st in steps" :key="st.n" :class="st.state">
          <span class="mark">{{ STEP[st.state] || st.n }}</span>
          <div>
            <strong>{{ st.title }}</strong>
            <p class="hint">{{ st.line }}</p>
          </div>
          <div v-if="st.n === 1 && !member?.signedIn" class="sign-buttons">
            <template v-if="libraryOpened || member?.expired">
              <button class="primary" :disabled="signing" @click="signIn">{{ signing ? 'Waiting for your browser…' : 'Connect Castle' }}</button>
              <button class="link" :disabled="signing" @click="signInOnLibrary">Open the Library again</button>
            </template>
            <template v-else>
              <button class="primary" @click="signInOnLibrary">Sign in</button>
              <button class="link" :disabled="signing" @click="signIn">{{ signing ? 'Waiting for your browser…' : 'Already signed in on the Library? Connect Castle' }}</button>
            </template>
          </div>
          <button v-if="st.n === 2 && member?.signedIn" @click="openLibrary">Open the Library</button>
        </li>
      </ol>
      <p v-if="doorProblem || member?.problem" class="problem">{{ doorProblem || member.problem }}</p>
      <p v-if="refused && !['notSetUp', 'locked', 'off'].includes(refused.kind)" class="problem">{{ refused.message }}</p>
      <div v-if="member?.signedIn" class="row">
        <span class="hint">Castle looks again by itself every half minute while you are here.</span>
        <span class="spacer"></span>
        <button @click="checkDoor">Check now</button>
        <button @click="signOut">Sign out</button>
      </div>
    </section>

    <template v-else>
      <section class="talk widget">
        <div v-if="me?.needsName" class="banner">Set your Foundation display name on the Portfolio to speak here. You can read already.</div>
        <div v-else-if="me?.muted || refused?.kind === 'muted'" class="banner">You are muted in this room for now. You can still read.</div>
        <div v-else-if="refused && !['locked', 'notSetUp'].includes(refused.kind)" class="banner">{{ refused.message }}</div>
        <div class="tabs">
          <button :class="{ on: tab === 'chat' }" @click="tab = 'chat'">CHAT</button>
          <button :class="{ on: tab === 'log' }" @click="tab = 'log'">LOG</button>
        </div>
        <p v-if="readProblem" class="problem">{{ readProblem }}</p>
        <div v-show="tab === 'chat'" ref="scroller" class="lines">
          <p v-if="!shown.length" class="hint">The room is quiet. Say hello.</p>
          <template v-for="item in shown" :key="item.key">
            <div v-if="item.kind === 'day'" class="day-divider"><time :datetime="item.key" :title="item.date">{{ item.label }}</time></div>
            <template v-else v-for="l in [item.line]" :key="l.id">
            <p v-if="l.author?.kind === 'system'" class="system"><em>{{ l.text }}</em></p>
            <div v-else-if="mine(l)" class="bubble mine">
              <span class="text">{{ l.text }}</span>
              <span class="meta">{{ clock(l.at) }} <span class="tick" :class="{ read: ticks(l, seen, me).read }" :title="ticks(l, seen, me).title">{{ ticks(l, seen, me).mark }}</span></span>
            </div>
            <div v-else class="bubble theirs" :class="[l.author?.kind, { ai: isAi(l) }]">
              <span class="face"><img v-if="faces[avatarToken(l.author)]" :src="faces[avatarToken(l.author)]" alt="" /><template v-else>{{ initial(l.author) }}</template></span>
              <div class="body">
                <span class="who" :title="speaker(l.author).title" :tabindex="isAi(l) ? 0 : undefined" :aria-label="speaker(l.author).title ? `${speaker(l.author).name}, ${speaker(l.author).title}` : undefined">{{ speaker(l.author).name }}<span v-if="isAi(l)" class="spark"> ✦</span></span>
                <span class="text">{{ l.text }}</span>
                <audio v-if="l.voice && files[l.voice.fileId]" :src="files[l.voice.fileId]" controls preload="none"></audio>
                <img v-if="l.picture && files[l.picture.fileId]" :src="files[l.picture.fileId]" alt="" class="picture" />
              </div>
              <span class="meta">{{ clock(l.at) }}</span>
            </div>
            </template>
          </template>
        </div>
        <div v-show="tab === 'log'" class="lines log">
          <p class="hint">{{ companion }}'s turns in the room, raw: what it was handed, and what it said or chose not to say. It answers from a room-only run that holds just its public card - it cannot open your memory or run anything.</p>
          <pre v-if="turnLog" class="turns">{{ turnLog }}</pre>
          <p v-else class="hint">No turns yet.</p>
        </div>
        <form class="say" @submit.prevent="send">
          <input v-model="draft" maxlength="1000" placeholder="Write to the room..." :disabled="me?.needsName || me?.muted" />
          <button class="primary" :disabled="!draft.trim()">Send</button>
        </form>
      </section>

      <aside class="side widget">
        <h3>IN THE ROOM</h3>
        <ul>
          <li v-for="(p, index) in presence" :key="`${p.kind}:${p.name}:${index}`" :class="presenceState(p)">
            <span class="face small"><img v-if="faces[p.avatar]" :src="faces[p.avatar]" alt="" /><template v-else>{{ initial(p) }}</template></span>
            <span class="dot" role="img" :aria-label="presenceState(p)" :title="presenceState(p)">●</span> <span :title="speaker(p).title" :tabindex="speaker(p).title ? 0 : undefined" :aria-label="speaker(p).title ? `${speaker(p).name}, ${speaker(p).title}` : undefined">{{ speaker(p).name }}<span v-if="['assistant', 'member_assistant'].includes(p.kind)"> ✦</span></span>
            <span v-if="p.kind === 'host'" class="tag">host</span>
            <span v-else-if="p.foundation" class="tag">Foundation</span>
          </li>
        </ul>
        <h3>{{ companion.toUpperCase() }} IN THE ROOM</h3>
        <button type="button" role="switch" :aria-checked="haveCompanionKey" :aria-label="`Let ${companion} in`" :disabled="roomSaving" class="room-toggle" @click="setCompanionIn(!haveCompanionKey)">
          <span>{{ roomSaving ? 'Updating…' : `Let ${companion} in` }}</span>
          <span class="toggle-track" :class="{ enabled: haveCompanionKey }" aria-hidden="true"><span class="toggle-thumb"></span></span>
        </button>
        <label for="room-participation" class="hint participation-label">Participation rule</label>
        <select id="room-participation" class="participation-select" :value="mode" :disabled="!haveCompanionKey || roomSaving" @change="changeParticipation($event.target.value)">
          <option value="off">Listening only</option>
          <option value="named">When named</option>
          <option value="joins">Joins in</option>
        </select>
        <p class="hint">{{ !haveCompanionKey ? `Switch on to let ${companion} enter, then choose a rule.` : mode === 'off' ? `${companion} listens without posting replies.` : mode === 'named' ? `${companion} replies when someone mentions its name.` : `${companion} may join the conversation on your usage, without sharing anything private.` }}</p>
        <p v-if="doorProblem" class="problem">{{ doorProblem }}</p>
        <label v-if="haveCompanionKey" class="mode"><input type="checkbox" :checked="settings?.emotionRoomVoice !== false" @change="setRoomVoice($event.target.checked)" /> Let {{ companion }}'s emotion engine colour how it talks here</label>
        <p v-if="haveCompanionKey" class="hint">Only how it sounds, never what it knows: its traits' one-line manner, and the ones meant for you only when it answers you.</p>
        <button v-if="haveCompanionKey" @click="card.open ? (card.open = false) : openCard()">{{ card.open ? 'Close the public card' : `${companion}'s public card` }}</button>
        <div v-if="card.open" class="card-edit">
          <p class="hint">All {{ companion }} may say about itself in the room. Nothing from your memory goes there.</p>
          <textarea v-model="card.text" rows="6" maxlength="2000"></textarea>
          <button class="primary" @click="saveCard">Save the card</button>
          <p v-if="card.saved" class="hint">{{ card.saved }}</p>
        </div>
        <h3>YOUR PROFILE</h3>
        <div class="profile-pair">
          <section class="profile-person" aria-label="Your room picture">
            <span class="profile-picture"><img v-if="myPicture" :src="myPicture" :alt="`${me?.name || member?.name || 'Your'} profile picture`" /><span v-else>{{ initial(me) }}</span></span>
            <strong :title="me?.name || member?.name">{{ me?.name || member?.name || 'You' }}</strong>
            <span class="hint">You</span>
            <button @click="setFace(false)">Change my picture</button>
            <button class="subtle" @click="setFace(false, true)">Use Google picture</button>
          </section>
          <section class="profile-person" :aria-label="`${companion}'s room picture`">
            <span class="profile-picture"><img v-if="companionPicture" :src="companionPicture" :alt="`${companion}'s profile picture`" /><span v-else>✦</span></span>
            <strong :title="companion">{{ companion }}</strong>
            <span class="hint">Companion</span>
            <button :disabled="!haveCompanionKey" :title="haveCompanionKey ? `Change ${companion}'s room picture` : `Let ${companion} in to change the room picture`" @click="setFace(true)">Change {{ companion }}'s picture</button>
          </section>
        </div>
        <p v-if="faceLine" role="status" class="hint">{{ faceLine }}</p>
        <button v-if="!nameEditing" @click="displayName = me?.name || member?.name || ''; nameEditing = true; nameLine = ''">Change display name</button>
        <form v-else class="name-edit" @submit.prevent="saveDisplayName">
          <label for="room-display-name">Public display name</label>
          <input id="room-display-name" v-model="displayName" maxlength="80" :disabled="nameSaving" />
          <p class="hint">Shown to everyone in the Foundation room. Leave empty to use your Library card name.</p>
          <div class="row wrap"><button class="primary" :disabled="nameSaving">{{ nameSaving ? 'Saving…' : 'Save name' }}</button><button type="button" :disabled="nameSaving" @click="nameEditing = false">Cancel</button></div>
        </form>
        <p v-if="nameLine" role="status" class="hint">{{ nameLine }}</p>
        <div class="profile-footer"><button @click="signOut">Sign out</button></div>
      </aside>
    </template>
  </div>
</template>

<style scoped>
.lines, .side {
  color-scheme: dark;
  scrollbar-width: thin;
  scrollbar-color: color-mix(in srgb, var(--text-dim) 45%, transparent) transparent;
  scrollbar-gutter: stable;
  overscroll-behavior: contain;
}
.room-toggle { display: flex; align-items: center; justify-content: space-between; gap: 12px; width: 100%; text-align: left; min-height: 44px; }
.toggle-track { display: inline-flex; align-items: center; width: 38px; height: 22px; flex-shrink: 0; padding: 3px; box-sizing: border-box; border-radius: 20px; background: var(--line); }
.toggle-thumb { width: 16px; height: 16px; border-radius: 50%; background: var(--text); transition: transform .15s ease; }
.toggle-track.enabled { background: var(--accent); }
.toggle-track.enabled .toggle-thumb { transform: translateX(16px); background: #07142e; }
.participation-label { margin-top: 4px; }
.participation-select { width: 100%; min-height: 40px; padding: 8px 10px; box-sizing: border-box; border: 1px solid var(--line); border-radius: 8px; color: var(--text); background: var(--panel-background); font: inherit; font-size: 13px; }
.participation-select:disabled { opacity: .45; cursor: not-allowed; }
.room-toggle:focus-visible, .participation-select:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
.profile-pair { display: grid; grid-template-columns: minmax(0, 1fr) minmax(0, 1fr); gap: 10px; }
.profile-person { display: flex; flex-direction: column; align-items: center; gap: 7px; min-width: 0; padding: 12px 8px; border: 1px solid var(--line); border-radius: 12px; background: rgba(255,255,255,.025); }
.profile-picture { display: grid; place-items: center; width: 64px; height: 64px; flex-shrink: 0; border-radius: 16px; overflow: hidden; background: color-mix(in srgb, var(--accent) 15%, transparent); color: var(--accent); font-size: 24px; }
.profile-picture img { width: 100%; height: 100%; object-fit: cover; }
.profile-person strong { width: 100%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; text-align: center; font-size: 13px; }
.profile-person button { width: 100%; min-height: 38px; padding: 7px 5px; font-size: 12px; line-height: 1.35; overflow-wrap: anywhere; }
.profile-person button.subtle { border-color: transparent; color: var(--text-dim); }
.profile-footer { padding-top: 12px; margin-top: 4px; border-top: 1px solid var(--line); }
.profile-footer button { width: 100%; }
.name-edit { display: flex; flex-direction: column; gap: 8px; }
.name-edit input { width: 100%; min-width: 0; box-sizing: border-box; padding: 8px; border: 1px solid var(--line); border-radius: 8px; color: var(--text); background: transparent; }
.day-divider { display: flex; align-items: center; gap: 12px; width: 100%; margin: 14px 0; color: var(--text-dim); font-size: 12px; }
.day-divider::before, .day-divider::after { content: ''; flex: 1; height: 1px; background: var(--line); }
.day-divider time { padding: 4px 10px; border-radius: 12px; background: var(--surface, rgba(255,255,255,.05)); }
.turns {
  margin: 0;
  font: 12px/1.5 ui-monospace, Consolas, monospace;
  white-space: pre-wrap;
  color: var(--text-dim);
}
.card-edit {
  display: flex;
  flex-direction: column;
  gap: 6px;
}
.card-edit textarea {
  padding: 8px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 13px;
  resize: vertical;
}
.tabs {
  display: flex;
  gap: 6px;
  margin-bottom: 8px;
}
.tabs button.on {
  border-color: var(--accent);
  color: var(--accent);
}
.bubble {
  display: flex;
  gap: 10px;
  align-items: flex-end;
  max-width: 78%;
  font-size: 14px;
}
.bubble.mine {
  align-self: flex-end;
  flex-direction: column;
  align-items: flex-end;
  gap: 2px;
  padding: 8px 12px;
  border-radius: 12px 12px 2px 12px;
  background: color-mix(in srgb, var(--accent) 22%, transparent);
}
.bubble.theirs .body {
  display: flex;
  flex-direction: column;
  gap: 2px;
  padding: 8px 12px;
  border-radius: 12px 12px 12px 2px;
  background: rgba(255, 255, 255, 0.06);
}
.face {
  display: grid;
  flex: none;
  place-items: center;
  width: 32px;
  height: 32px;
  overflow: hidden;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.12);
  font-size: 14px;
  font-weight: 600;
}
.face img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}
.face.small {
  display: inline-grid;
  width: 22px;
  height: 22px;
  margin-right: 4px;
  font-size: 11px;
  vertical-align: middle;
}
.meta {
  font-size: 11px;
  color: var(--text-dim);
}
.tick {
  letter-spacing: -2px;
}
.tick.read {
  color: #53bdeb;
}
.system {
  align-self: center;
  margin: 0;
  color: var(--good);
  font-size: 13px;
}
.row.wrap {
  flex-wrap: wrap;
}
.steps {
  display: grid;
  gap: 12px;
  margin: 4px 0;
  padding: 0;
  list-style: none;
}
.steps li {
  display: grid;
  grid-template-columns: 32px 1fr auto;
  gap: 12px;
  align-items: center;
}
.steps .mark {
  display: grid;
  place-items: center;
  width: 28px;
  height: 28px;
  border: 1px solid var(--line);
  border-radius: 50%;
  font-size: 13px;
}
.steps li.done .mark {
  background: var(--good);
  border-color: var(--good);
  color: #07142e;
}
.steps li.wait .mark {
  border-color: var(--accent);
  color: var(--accent);
}
.row {
  display: flex;
  gap: 8px;
  align-items: center;
}
.spacer {
  flex: 1;
}
.chat-room {
  position: absolute;
  inset: 24px;
  display: flex;
  gap: 16px;
}
.widget {
  background: var(--panel-background);
  border: 1px solid var(--line);
  border-radius: 14px;
  backdrop-filter: blur(var(--glass-blur));
}
.door {
  display: flex;
  flex-direction: column;
  gap: 12px;
  width: min(620px, 100%);
  margin: auto;
  padding: 28px 32px;
}
.eyebrow {
  margin: 0;
  font-size: 11px;
  letter-spacing: 0.3em;
  color: var(--accent);
}
h2 {
  margin: 0;
  font-size: 22px;
  font-weight: 500;
}
h3 {
  margin: 4px 0 6px;
  font-size: 12px;
  font-weight: 500;
  letter-spacing: 0.14em;
}
label {
  display: flex;
  flex-direction: column;
  gap: 6px;
  font-size: 13px;
}
label.mode {
  flex-direction: row;
  align-items: center;
  gap: 8px;
}
input:not([type='radio']) {
  padding: 8px 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: rgba(0, 0, 0, 0.25);
  color: var(--text);
  font: inherit;
  font-size: 14px;
}
.talk {
  display: flex;
  flex: 1;
  flex-direction: column;
  min-width: 0;
  padding: 14px 18px;
}
.banner {
  margin-bottom: 8px;
  padding: 8px 12px;
  border: 1px solid var(--warn);
  border-radius: 10px;
  font-size: 13px;
}
.lines {
  display: flex;
  flex: 1;
  flex-direction: column;
  gap: 8px;
  overflow-y: auto;
  padding-right: 6px;
}
.line {
  display: grid;
  grid-template-columns: 170px 1fr auto;
  gap: 4px 12px;
  font-size: 14px;
}
.line.system {
  display: block;
  color: var(--good);
  font-size: 13px;
}
.who {
  color: var(--text-dim);
}
.line.ai .who {
  color: var(--accent);
}
.line.host .who {
  color: #ffd59a;
}
.text {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.line audio,
.line .picture {
  grid-column: 2;
  max-width: 320px;
  border-radius: 8px;
}
.at {
  font-size: 12px;
  color: var(--text-dim);
}
.say {
  display: flex;
  gap: 8px;
  margin-top: 10px;
}
.say input {
  flex: 1;
}
.side {
  display: flex;
  flex-direction: column;
  gap: 8px;
  width: 280px;
  padding: 16px;
  min-height: 0;
  overflow-y: auto;
}
ul {
  display: grid;
  gap: 6px;
  margin: 0 0 8px;
  padding: 0;
  list-style: none;
  font-size: 14px;
}
.dot {
  color: var(--text-dim);
}
li.online .dot, li.here .dot, li.active .dot {
  color: var(--good);
}
.tag {
  margin-left: 6px;
  font-size: 11px;
  color: var(--text-dim);
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
button {
  padding: 7px 14px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}
.sign-buttons {
  display: flex;
  flex-direction: column;
  align-items: flex-end;
  gap: 4px;
}
.sign-buttons button.link {
  background: none;
  border: none;
  padding: 0;
  font-size: 12px;
  opacity: 0.8;
  text-decoration: underline;
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
