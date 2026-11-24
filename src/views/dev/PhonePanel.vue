<script setup>
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { mobileCheck, onPhoneExit, onPhoneOutput, phoneKey, phoneReload, phoneRestart, phoneRotate, phoneRun, phoneRunInfo, phoneRunStop, phoneScreen, phoneScreenEnd, phoneStart, phoneState, phoneStop, phoneTouch } from '../../api/castle.js'
import MobileSetup from './MobileSetup.vue'
import { deviceSize, frameWidth, iosAllowed, keepTail, parseFrame, servedIn, stateLine, toDevice } from './mobile.js'

// A DEV tab's Phone panel: Mobile
// setup while anything is missing or the phone is off, the docked phone once it runs - its screen
// drawn from the emulator's own stream, a click a touch, typing typed into it.
const props = defineProps({
  // the DEV tab's slot
  index: { type: Number, required: true },
  // the panel is showing
  shown: { type: Boolean, default: false },
  // the companion's session in this tab is up, and the way to type into it (Ask my companion)
  running: { type: Boolean, default: false },
  say: { type: Function, required: true },
  // the companion is answering right now: after an ask, the end of its reply re-checks the setup
  busy: { type: Boolean, default: false },
})

// Ask my companion: the setup is read again when the companion's reply ends (busy went true, then false)
let asked = false
let heardBusy = false
function onAsked() {
  asked = true
  heardBusy = false
}
watch(
  () => props.busy,
  (now) => {
    if (!asked) return
    if (now) heardBusy = true
    else if (heardBusy) {
      asked = false
      recheck()
    }
  },
)

const check = ref(null)
const problem = ref('')
const platform = ref('android')
const chosen = ref('')
const phone = ref({ state: 'off' })
const canvas = ref(null)
const screenBox = ref(null)
const hasFrame = ref(false)
let frame = null // the newest frame's size
let pollTimer
const run = ref({ running: false, kind: '', target: '', preview: '' })
const lines = ref([])
const linesBox = ref(null)
const stops = []
let streaming = false
let pressed = false
let lastMove = 0

const on = computed(() => ['starting', 'booting', 'ready'].includes(phone.value.state))
const ready = computed(() => phone.value.state === 'ready')
const chosenPhone = computed(() => (check.value?.phones ?? []).find((p) => p.name === (phone.value.avd || chosen.value)) ?? null)
const ios = computed(() => iosAllowed(check.value?.kind))
const isFlutter = computed(() => check.value?.kind === 'flutter')
// the iPhone preview: the Flutter app as a local web page in an iPhone frame, shown once served
const previewReady = ref(false)
const previewing = computed(() => platform.value === 'ios' && run.value.running && run.value.target === 'ios')
const showSetup = computed(() => (platform.value === 'ios' ? !previewing.value : !on.value || !check.value?.ready))

// Run, Hot reload, Restart: the app built and put on the phone in its own console
async function runApp() {
  problem.value = ''
  previewReady.value = false
  try {
    run.value = await phoneRun(props.index, platform.value === 'ios')
  } catch (reason) {
    problem.value = String(reason)
  }
}
async function reloadApp() {
  try {
    await phoneReload(props.index)
  } catch (reason) {
    problem.value = String(reason)
  }
}
async function restartApp() {
  try {
    run.value = await phoneRestart(props.index)
  } catch (reason) {
    problem.value = String(reason)
  }
}
async function stopApp() {
  try {
    await phoneRunStop(props.index)
  } catch (reason) {
    problem.value = String(reason)
  }
}
function heard(text) {
  lines.value = keepTail(lines.value, text)
  if (servedIn(text)) previewReady.value = true
  nextTick(() => linesBox.value && (linesBox.value.scrollTop = linesBox.value.scrollHeight))
}

async function recheck() {
  try {
    check.value = await mobileCheck(props.index)
    problem.value = ''
    const names = (check.value.phones ?? []).map((p) => p.name)
    if (!names.includes(chosen.value)) chosen.value = names[0] ?? ''
  } catch (reason) {
    problem.value = String(reason)
  }
}

async function readState() {
  try {
    phone.value = await phoneState(props.index)
  } catch {
    return
  }
  if (ready.value && props.shown && !streaming) startScreen()
  if (phone.value.state === 'failed') problem.value = phone.value.note
  if (!on.value) {
    stopPolling()
    streaming = false
  }
}

function startPolling() {
  stopPolling()
  pollTimer = setInterval(readState, 1000)
}
function stopPolling() {
  clearInterval(pollTimer)
  pollTimer = null
}

async function start() {
  if (!chosen.value) return
  problem.value = ''
  try {
    phone.value = await phoneStart(props.index, chosen.value)
    startPolling()
  } catch (reason) {
    problem.value = String(reason)
  }
}

async function stop() {
  await stopApp()
  stopScreen()
  try {
    await phoneStop(props.index)
  } catch (reason) {
    problem.value = String(reason)
  }
  hasFrame.value = false
  phone.value = { state: 'off' }
}

function draw(buffer) {
  const f = parseFrame(buffer)
  const c = canvas.value
  if (!f || !c) return
  if (c.width !== f.width || c.height !== f.height) {
    c.width = f.width
    c.height = f.height
  }
  c.getContext('2d').putImageData(new ImageData(f.pixels, f.width, f.height), 0, 0)
  frame = { width: f.width, height: f.height }
  hasFrame.value = true
}

async function startScreen() {
  streaming = true
  await nextTick()
  const width = frameWidth(screenBox.value?.clientWidth || 300, window.devicePixelRatio)
  try {
    await phoneScreen(props.index, width, draw, (why) => {
      streaming = false
      problem.value = why
    })
  } catch (reason) {
    streaming = false
    problem.value = String(reason)
  }
}

function stopScreen() {
  if (!streaming) return
  streaming = false
  phoneScreenEnd(props.index).catch(() => {})
}

// a click is a finger on the phone: down, moved, up
function pointAt(event) {
  const c = canvas.value
  if (!c || !frame) return null
  const rect = c.getBoundingClientRect()
  return toDevice(event.clientX - rect.left, event.clientY - rect.top, rect, deviceSize(chosenPhone.value, frame))
}
function down(event) {
  const p = pointAt(event)
  if (!p || !ready.value) return
  pressed = true
  canvas.value.setPointerCapture?.(event.pointerId)
  canvas.value.focus()
  phoneTouch(props.index, p.x, p.y, true).catch(() => {})
}
function move(event) {
  if (!pressed || Date.now() - lastMove < 16) return
  const p = pointAt(event)
  if (!p) return
  lastMove = Date.now()
  phoneTouch(props.index, p.x, p.y, true).catch(() => {})
}
function up(event) {
  if (!pressed) return
  pressed = false
  const p = pointAt(event)
  if (p) phoneTouch(props.index, p.x, p.y, false).catch(() => {})
}

// typing while the phone has focus goes to the phone
function typed(event) {
  if (!ready.value || event.ctrlKey || event.metaKey || event.altKey) return
  if (event.key === 'Enter') phoneKey(props.index, 'enter').catch(() => {})
  else if (event.key === 'Backspace') phoneKey(props.index, 'backspace').catch(() => {})
  else if (event.key.length === 1) phoneKey(props.index, null, event.key).catch(() => {})
  else return
  event.preventDefault()
}

function button(name) {
  if (!ready.value) return
  phoneKey(props.index, name).catch((reason) => (problem.value = String(reason)))
}
function rotate() {
  if (!ready.value) return
  phoneRotate(props.index).catch((reason) => (problem.value = String(reason)))
}

watch(
  () => props.shown,
  (shown) => {
    if (shown) {
      recheck()
      readState()
      if (on.value) startPolling()
    } else {
      stopScreen()
    }
  },
)
onMounted(async () => {
  stops.push(await onPhoneOutput(props.index)(heard))
  stops.push(await onPhoneExit(props.index)(() => (run.value = { ...run.value, running: false })))
  try {
    const info = await phoneRunInfo(props.index)
    run.value = info
    lines.value = keepTail([], info.tail)
    if (info.running && info.target === 'ios') {
      platform.value = 'ios'
      previewReady.value = servedIn(info.tail)
    }
  } catch {
    // nothing ran yet
  }
  await recheck()
  await readState()
  if (on.value) startPolling()
})
onBeforeUnmount(() => {
  stops.forEach((stopListening) => stopListening())
  stopPolling()
  stopScreen()
})
</script>

<template>
  <aside class="phone-panel" aria-label="Phone">
    <p v-if="problem" class="problem">{{ problem }}</p>

    <MobileSetup
      v-if="showSetup"
      :check="check"
      :platform="platform"
      :chosen="chosen"
      :running="running"
      :say="say"
      @recheck="recheck"
      @platform="platform = $event"
      @choose="chosen = $event"
      @run="platform === 'ios' ? runApp() : start()"
      @asked="onAsked"
    />

    <template v-else-if="platform === 'ios'">
      <div class="bar">
        <div class="switch" role="group" aria-label="Android or iOS">
          <button type="button" @click="platform = 'android'">Android</button>
          <button type="button" class="on">iOS</button>
        </div>
        <span class="which">iPhone preview</span>
      </div>
      <div class="runs">
        <button type="button" :disabled="!run.running" title="Hot reload (r)" @click="reloadApp">&#9889; Hot reload</button>
        <button type="button" :disabled="!run.running" title="Hot restart (R)" @click="restartApp">&#8635; Restart</button>
        <button type="button" :disabled="!run.running" @click="stopApp">&#9632; Stop</button>
      </div>
      <div class="iphone">
        <span class="notch" aria-hidden="true"></span>
        <div class="viewport">
          <!-- drawn at an iPhone's own 390 x 844 points, shown scaled into the frame -->
          <iframe v-if="previewReady" :src="run.preview" title="iPhone preview" class="web"></iframe>
          <p v-else class="waiting">Building the web preview…</p>
        </div>
        <span class="bar-home" aria-hidden="true"></span>
      </div>
      <p class="state">iPhone preview - not a real iPhone. The real iPhone simulator needs a Mac.</p>
      <pre v-if="lines.length" class="lines">{{ lines.join(String.fromCharCode(10)) }}</pre>
    </template>

    <template v-else>
      <div class="bar">
        <div class="switch" role="group" aria-label="Android or iOS">
          <button type="button" :class="{ on: platform === 'android' }" @click="platform = 'android'">Android</button>
          <button type="button" :class="{ on: platform === 'ios' }" :disabled="!ios" :title="ios ? '' : 'Kotlin builds Android apps only.'" @click="platform = 'ios'">iOS</button>
        </div>
        <span class="which" :title="phone.serial">{{ chosenPhone?.label || phone.avd }}</span>
        <button type="button" class="small" title="Stop this phone" @click="stop">Stop phone</button>
      </div>
      <div class="runs">
        <button type="button" class="go" :disabled="!ready || run.running" @click="runApp">&#9654; Run</button>
        <button type="button" :disabled="!run.running || !isFlutter" :title="isFlutter ? 'Hot reload (r)' : 'Kotlin has no hot reload from the command line - Restart reinstalls.'" @click="reloadApp">&#9889; Hot reload</button>
        <button type="button" :disabled="!ready || (isFlutter && !run.running)" :title="isFlutter ? 'Hot restart (R)' : 'Build, install and open again'" @click="restartApp">&#8635; Restart</button>
        <button type="button" :disabled="!run.running" @click="stopApp">&#9632; Stop</button>
      </div>

      <div ref="screenBox" class="frame">
        <canvas
          ref="canvas"
          class="screen"
          :class="{ blank: !hasFrame }"
          tabindex="0"
          aria-label="The phone's screen - click to touch, type to type"
          @pointerdown="down"
          @pointermove="move"
          @pointerup="up"
          @pointercancel="up"
          @keydown="typed"
        ></canvas>
        <p v-if="!hasFrame" class="waiting">{{ stateLine(phone) }}</p>
      </div>

      <div class="buttons">
        <button type="button" title="Back" :disabled="!ready" @click="button('back')">&#9665;</button>
        <button type="button" title="Home" :disabled="!ready" @click="button('home')">&#9711;</button>
        <button type="button" title="Recent apps" :disabled="!ready" @click="button('recent')">&#9634;</button>
        <button type="button" title="Rotate" :disabled="!ready" @click="rotate">&#10227;</button>
      </div>
      <p class="state"><span class="dot" :class="phone.state"></span>{{ stateLine(phone) }}</p>
      <pre v-if="lines.length && platform === 'android'" ref="linesBox" class="lines">{{ lines.join(String.fromCharCode(10)) }}</pre>
    </template>
  </aside>
</template>

<style scoped>
.phone-panel {
  display: flex;
  flex: none;
  flex-direction: column;
  gap: 10px;
  width: 420px;
  min-height: 0;
  padding: 12px 14px;
  overflow-y: auto;
  border-left: 1px solid rgba(198, 162, 255, 0.2);
  font-size: 13px;
}

.problem {
  margin: 0;
  color: #ffb3b3;
}

.bar {
  display: flex;
  align-items: center;
  gap: 10px;
}

.which {
  flex: 1;
  overflow: hidden;
  color: var(--text-dim);
  white-space: nowrap;
  text-overflow: ellipsis;
}

.switch {
  display: flex;
  border: 1px solid rgba(198, 162, 255, 0.3);
  border-radius: 999px;
  overflow: hidden;
}

.switch button {
  padding: 4px 14px;
  border: 0;
  background: none;
  color: var(--text-dim);
  font: inherit;
  font-size: 12.5px;
  cursor: pointer;
}

.switch button.on {
  background: color-mix(in srgb, var(--accent, #b48cff) 70%, transparent);
  color: #fff;
}

.switch button:disabled {
  opacity: 0.4;
  cursor: default;
}

.frame {
  position: relative;
  align-self: center;
  width: 300px;
  padding: 10px 8px;
  border: 2px solid #0d0b14;
  border-radius: 34px;
  background: #0d0b14;
  box-shadow: 0 0 0 1px rgba(198, 162, 255, 0.25), 0 10px 30px rgba(0, 0, 0, 0.5);
}

.screen {
  display: block;
  width: 100%;
  height: auto;
  border-radius: 24px;
  background: #000;
  cursor: pointer;
  outline: none;
  touch-action: none;
}

.screen.blank {
  aspect-ratio: 9 / 20;
}

.screen:focus-visible {
  box-shadow: 0 0 0 2px var(--accent, #b48cff);
}

.waiting {
  position: absolute;
  inset: 0;
  display: grid;
  margin: 0;
  place-items: center;
  color: var(--text-dim);
  pointer-events: none;
}

.buttons {
  display: flex;
  justify-content: center;
  gap: 14px;
}

.buttons button {
  width: 38px;
  height: 30px;
  border: 1px solid rgba(198, 162, 255, 0.3);
  border-radius: 999px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 15px;
  cursor: pointer;
}

.buttons button:disabled {
  opacity: 0.4;
  cursor: default;
}

.state {
  display: flex;
  align-items: center;
  justify-content: center;
  gap: 8px;
  margin: 0;
  color: var(--text-dim);
}

.dot {
  width: 8px;
  height: 8px;
  border-radius: 50%;
  background: #f0b44c;
}

.dot.ready {
  background: #5fd38a;
}

.dot.failed {
  background: #ff7a7a;
}

.runs {
  display: flex;
  gap: 6px;
}

.runs button {
  flex: 1;
  padding: 6px 4px;
  border: 1px solid rgba(198, 162, 255, 0.3);
  border-radius: 999px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 12.5px;
  white-space: nowrap;
  cursor: pointer;
}

.runs .go {
  border: 0;
  background: color-mix(in srgb, var(--accent, #b48cff) 75%, transparent);
  color: #fff;
}

.runs button:disabled {
  opacity: 0.4;
  cursor: default;
}

.iphone {
  position: relative;
  align-self: center;
  width: 300px;
  padding: 12px 10px;
  border-radius: 46px;
  background: #0d0b14;
  box-shadow: 0 0 0 1px rgba(198, 162, 255, 0.25), 0 10px 30px rgba(0, 0, 0, 0.5);
}

.viewport {
  position: relative;
  width: 280px;
  height: 606px;
  overflow: hidden;
  border-radius: 36px;
  background: #fff;
}

.iphone .web {
  width: 390px;
  height: 844px;
  border: 0;
  transform: scale(0.7179);
  transform-origin: 0 0;
}

.notch {
  position: absolute;
  top: 20px;
  left: 50%;
  z-index: 1;
  width: 90px;
  height: 24px;
  border-radius: 999px;
  background: #0d0b14;
  transform: translateX(-50%);
}

.bar-home {
  position: absolute;
  bottom: 18px;
  left: 50%;
  width: 110px;
  height: 4px;
  border-radius: 999px;
  background: rgba(0, 0, 0, 0.45);
  transform: translateX(-50%);
}

.lines {
  max-height: 180px;
  margin: 0;
  padding: 8px 10px;
  overflow: auto;
  border: 1px solid rgba(198, 162, 255, 0.15);
  border-radius: 10px;
  background: rgba(0, 0, 0, 0.35);
  color: var(--text-dim);
  font-family: Consolas, 'Cascadia Mono', monospace;
  font-size: 11.5px;
  white-space: pre-wrap;
  word-break: break-word;
}

.small {
  padding: 3px 12px;
  border: 1px solid rgba(198, 162, 255, 0.3);
  border-radius: 999px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}
</style>
