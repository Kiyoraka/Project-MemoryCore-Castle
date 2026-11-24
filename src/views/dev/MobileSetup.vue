<script setup>
import { computed, ref } from 'vue'
import { mobileCreatePhone } from '../../api/castle.js'
import { bestImage, companionSentence, iosAllowed, newPhoneName, phoneLine, rowMark } from './mobile.js'

// DEV > Phone > Mobile setup: what this computer has for the
// project's phone - the Android SDK, the emulator, a phone, Flutter (or Java for Kotlin) - with
// Create for a phone (made here, from an installed image) and Install for the rest, which asks this
// tab's companion to install it ("just like how we do with database", his word 4:47 PM). Then the
// phones to pick, and Run.
const props = defineProps({
  check: { type: Object, default: null },
  platform: { type: String, default: 'android' },
  chosen: { type: String, default: '' },
  // the companion's session in this tab is up (it can only be asked then)
  running: { type: Boolean, default: false },
  say: { type: Function, required: true },
})
const emit = defineEmits(['recheck', 'platform', 'choose', 'run', 'asked'])

const busy = ref('')
const note = ref('')
// a Mac's iOS side has its own rows (Xcode, a simulator); elsewhere iOS is the preview of the Android rows' Flutter
const macIos = computed(() => props.platform === 'ios' && (props.check?.ios?.length ?? 0) > 0)
const rows = computed(() => (macIos.value ? props.check.ios : props.check?.rows ?? []))
const ready = computed(() => (macIos.value ? props.check?.iosReady : props.check?.ready))
const phones = computed(() => props.check?.phones ?? [])
const missing = computed(() => companionSentence(rows.value))
const ios = computed(() => iosAllowed(props.check?.kind))

function ask(only = '') {
  const sentence = companionSentence(rows.value, only)
  if (!sentence) return
  if (!props.running) {
    note.value = 'Wake your companion first - it installs from its session in this tab.'
    return
  }
  props.say(sentence)
  emit('asked')
  note.value = 'Asked - this checks again by itself when your companion finishes.'
}

async function create() {
  const image = bestImage(props.check?.images)
  if (!image) return ask('phone')
  busy.value = 'create'
  note.value = 'Making a phone…'
  try {
    await mobileCreatePhone(newPhoneName(phones.value), image)
    note.value = ''
    emit('recheck')
  } catch (reason) {
    note.value = String(reason)
  } finally {
    busy.value = ''
  }
}
</script>

<template>
  <div class="setup">
    <section class="card">
      <header>
        <h4>Mobile setup</h4>
        <div class="switch" role="group" aria-label="Android or iOS">
          <button type="button" :class="{ on: platform === 'android' }" @click="emit('platform', 'android')">Android</button>
          <button type="button" :class="{ on: platform === 'ios' }" :disabled="!ios" :title="ios ? '' : 'Kotlin builds Android apps only.'" @click="emit('platform', 'ios')">iOS</button>
        </div>
      </header>

      <ul class="rows">
        <li v-for="r in rows" :key="r.id" :class="rowMark(r)">
          <span class="mark" aria-hidden="true">{{ rowMark(r) === 'ok' ? '✓' : '' }}</span>
          <span class="label">{{ r.label }}</span>
          <small class="detail" :title="r.detail">{{ r.detail }}</small>
          <button v-if="r.action === 'create'" type="button" class="small" :disabled="!!busy" @click="create">{{ busy === 'create' ? 'Making…' : 'Create' }}</button>
          <button v-else-if="r.action === 'install'" type="button" class="small" @click="ask(r.id)">Install</button>
        </li>
      </ul>
      <p v-if="macIos" class="ios">The real iPhone simulator, in its own window. Your Flutter app runs on it.</p>
      <p v-else-if="platform === 'ios'" class="ios">iPhone preview - your Flutter app in an iPhone-sized frame. The real iPhone simulator needs a Mac.</p>

      <button v-if="missing" type="button" class="ask" @click="ask()">&#10022; Ask my companion to install</button>
      <div class="foot">
        <small v-if="note" class="note">{{ note }}</small>
        <button type="button" class="link" @click="emit('recheck')">Check again</button>
      </div>
    </section>

    <div v-if="phones.length && !macIos && platform === 'android'" class="phones">
      <button v-for="p in phones" :key="p.name" type="button" class="phone" :class="{ on: chosen === p.name }" @click="emit('choose', p.name)">
        <span class="shape" aria-hidden="true"></span>
        <span class="words">
          <strong>{{ p.label }}</strong>
          <small>{{ phoneLine(p) }}</small>
        </span>
      </button>
    </div>
    <button v-if="ready" type="button" class="run" :disabled="!chosen && platform === 'android'" @click="emit('run')">Run</button>
  </div>
</template>

<style scoped>
.setup {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.card {
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 14px 16px;
  border: 1px solid rgba(198, 162, 255, 0.25);
  border-radius: 14px;
  background: rgba(20, 14, 40, 0.45);
}

.card header {
  display: flex;
  align-items: center;
  justify-content: space-between;
}

h4 {
  margin: 0;
  font-family: Georgia, 'Cambria', serif;
  font-size: 20px;
  font-weight: 500;
  color: var(--text);
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

.rows {
  display: flex;
  flex-direction: column;
  margin: 0;
  padding: 0;
  list-style: none;
}

.rows li {
  display: grid;
  grid-template-columns: 22px 92px minmax(0, 1fr) auto;
  align-items: center;
  gap: 10px;
  padding: 9px 2px;
  border-bottom: 1px solid rgba(198, 162, 255, 0.12);
}

.mark {
  display: grid;
  width: 18px;
  height: 18px;
  place-items: center;
  border: 2px solid rgba(198, 162, 255, 0.45);
  border-radius: 50%;
  font-size: 11px;
  color: #10230f;
}

.ok .mark {
  border-color: #5fd38a;
  background: #5fd38a;
}

.create .mark {
  border-color: #f0b44c;
}

.label {
  color: var(--text);
}

.detail {
  overflow: hidden;
  color: var(--text-dim);
  white-space: nowrap;
  text-overflow: ellipsis;
}

.ios {
  margin: 0;
  color: var(--text-dim);
  font-size: 12.5px;
}

.ask {
  padding: 9px 12px;
  border: 0;
  border-radius: 10px;
  background: color-mix(in srgb, var(--accent, #b48cff) 75%, transparent);
  color: #fff;
  font: inherit;
  cursor: pointer;
  box-shadow: 0 0 14px color-mix(in srgb, var(--accent, #b48cff) 45%, transparent);
}

.foot {
  display: flex;
  align-items: center;
  justify-content: flex-end;
  gap: 10px;
}

.note {
  margin-right: auto;
  color: var(--text-dim);
}

.phones {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  gap: 10px;
}

.phone {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 10px;
  border: 1px solid rgba(198, 162, 255, 0.2);
  border-radius: 12px;
  background: rgba(20, 14, 40, 0.35);
  color: var(--text);
  font: inherit;
  text-align: left;
  cursor: pointer;
}

.phone.on {
  border-color: var(--accent, #b48cff);
  box-shadow: 0 0 12px color-mix(in srgb, var(--accent, #b48cff) 45%, transparent);
}

.shape {
  flex: none;
  width: 18px;
  height: 32px;
  border: 1.5px solid rgba(198, 162, 255, 0.6);
  border-radius: 4px;
}

.words {
  display: flex;
  flex-direction: column;
  min-width: 0;
}

.words strong {
  overflow: hidden;
  font-weight: 500;
  white-space: nowrap;
  text-overflow: ellipsis;
}

.words small {
  color: var(--text-dim);
  font-size: 11.5px;
}

.run {
  align-self: center;
  padding: 7px 34px;
  border: 1px solid #e9b75a;
  border-radius: 999px;
  background: none;
  color: #ffe2a3;
  font-family: Georgia, 'Cambria', serif;
  font-size: 15px;
  cursor: pointer;
}

.run:disabled {
  opacity: 0.45;
  cursor: default;
}

.small,
.link {
  padding: 3px 12px;
  border: 1px solid rgba(198, 162, 255, 0.3);
  border-radius: 999px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}

.link {
  border: 0;
  color: var(--text-dim);
  text-decoration: underline;
}
</style>
