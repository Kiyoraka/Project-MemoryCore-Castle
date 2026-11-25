<script setup>
// Settings > Keys: each key masked, with Show, Test, Replace and Remove; the room keys are never shown here.
// Cut from SettingsView.vue (Shard): the page's shared state comes through SETTINGS_PAGE.
import { computed, inject, ref } from 'vue'
import SettingGroup from '../../components/settings/SettingGroup.vue'
import SettingRow from '../../components/settings/SettingRow.vue'
import SettingConfirm from '../../components/settings/SettingConfirm.vue'
import { keyForget, keyReveal, keySave, keyTest } from '../../api/castle.js'
import { SETTINGS_PAGE } from './settingsPage.js'

const { keys, locked, problem, done, attempt, rows, words } = inject(SETTINGS_PAGE)

const drafts = ref({})
const shown = ref({})
const tests = ref({})
const replacing = ref({})

// ---------- Keys ----------

// the room sign-in and the companion's room key are made by CHAT itself, never shown or pasted here
const shownKeys = computed(() => keys.value.filter((k) => k.name !== 'member' && !k.name.startsWith('room')))

async function saveKey(name) {
  const value = (drafts.value[name] ?? '').trim()
  if (!value) return
  const next = await attempt('Keeping the key safe…', () => keySave(name, value))
  if (next) {
    keys.value = next
    drafts.value[name] = ''
    shown.value[name] = ''
    replacing.value[name] = false
    rows.saved(`key-${name}`)
    done.value = 'Key kept. Wake your companion again to use it.'
  }
}

function cancelKey(name) {
  drafts.value[name] = ''
  replacing.value[name] = false
}

async function toggleShow(name) {
  shown.value[name] = shown.value[name] ? '' : (await keyReveal(name)) ?? ''
}

async function test(name) {
  tests.value[name] = { ok: null, message: 'Checking…' }
  tests.value[name] = (await attempt('', () => keyTest(name))) ?? { ok: false, message: problem.value }
}

async function remove(name) {
  const next = await attempt('Removing…', () => keyForget(name))
  if (next) keys.value = next
}
</script>

<template>
  <SettingGroup id="keys" title="Keys" :note="`Kept ${words.keysKept}`" tip="Keys are only handed to your companion's session. Without a key, your companion saves the idea instead and Castle tells you. Room keys for the MemoryCore Foundation room are set at the CHAT room's door.">
    <SettingRow
      v-for="k in shownKeys"
      :id="`key-${k.name}`"
      :key="k.name"
      :label="k.label"
      :tip="k.name === 'openai' ? 'APPEARANCE and STUDIO draw on your ChatGPT plan whenever Codex is signed in with ChatGPT on this computer, whichever CLI your companion thinks with; otherwise with this key. The key is never given to Codex.' : ''"
    >
      <template #description>
        <code v-if="k.set" class="masked">{{ shown[k.name] || k.masked }}</code>
        <span v-else>Not set</span>
      </template>
      <template v-if="k.set">
        <button @click="toggleShow(k.name)">{{ shown[k.name] ? 'Hide' : 'Show' }}</button>
        <button @click="test(k.name)">Test</button>
        <button v-if="!replacing[k.name]" @click="replacing[k.name] = true">Replace</button>
        <SettingConfirm label="Remove" :disabled="locked" @confirm="remove(k.name)" />
      </template>
      <button v-else-if="!replacing[k.name]" class="primary" @click="replacing[k.name] = true">Add</button>
      <template v-if="replacing[k.name] || tests[k.name]" #below>
        <div v-if="replacing[k.name]" class="row key-paste">
          <input v-model="drafts[k.name]" type="password" :aria-label="`${k.label} key`" :placeholder="k.set ? 'Paste a new key to replace' : 'Paste key'" autocomplete="off" @keydown.enter="saveKey(k.name)" />
          <button @click="cancelKey(k.name)">Cancel</button>
          <button class="primary" :disabled="locked || !(drafts[k.name] ?? '').trim()" @click="saveKey(k.name)">Save</button>
        </div>
        <p v-if="tests[k.name]" :class="tests[k.name].ok === false ? 'warn' : 'good'">{{ tests[k.name].message }}</p>
      </template>
    </SettingRow>
  </SettingGroup>
</template>

<style scoped src="./settings-page.css"></style>
