<script setup>
// The browser's Passwords sheet, the Keep's way: search the saved logins, import Chrome's export, add
// a login by hand, copy a password for 30 seconds, delete one. A password is never shown and never
// reaches a page; the pages step aside while the sheet is open.
import { computed, onMounted, ref } from 'vue'
import { vaultAdd, vaultCopy, vaultForget, vaultImport, vaultList } from '../api/castle.js'
import { filterLogins } from './browserview.js'

const emit = defineEmits(['close'])
const logins = ref([])
const search = ref('')
const adding = ref(false)
const form = ref({ url: '', user: '', pass: '' })
const said = ref('')
const busy = ref(false)
const shown = computed(() => filterLogins(logins.value, search.value))

async function read() {
  try {
    logins.value = await vaultList()
  } catch (reason) {
    said.value = String(reason)
  }
}
async function run(work) {
  busy.value = true
  said.value = ''
  try {
    await work()
  } catch (reason) {
    said.value = String(reason)
  } finally {
    busy.value = false
  }
}
const importChrome = () =>
  run(async () => {
    const done = await vaultImport()
    if (!done) return
    said.value = `${done.sealed} sealed${done.skipped ? `, ${done.skipped} skipped` : ''}. ${done.shredded ? 'The exported file is shredded.' : 'Delete the exported file yourself: it could not be shredded.'}`
    await read()
  })
const seal = () =>
  run(async () => {
    await vaultAdd(form.value.url.trim(), form.value.user.trim(), form.value.pass)
    form.value = { url: '', user: '', pass: '' }
    adding.value = false
    said.value = 'Sealed.'
    await read()
  })
const copy = (login) =>
  run(async () => {
    await vaultCopy(login.id)
    said.value = `${login.host}'s password is on the clipboard for 30 seconds.`
  })
const remove = (login) =>
  run(async () => {
    await vaultForget(login.id)
    await read()
  })
onMounted(read)
</script>

<template>
  <aside class="sheet" role="dialog" aria-label="Passwords">
    <header>
      <span class="tag">BROWSER</span>
      <h2>Passwords</h2>
      <button class="close" aria-label="Close" @click="emit('close')">×</button>
    </header>
    <div class="row">
      <input v-model="search" class="search" placeholder="Search sites" aria-label="Search sites" />
      <button :disabled="busy" @click="importChrome">Import from Chrome</button>
      <button :disabled="busy" @click="adding = !adding">{{ adding ? 'Not now' : 'Add a login' }}</button>
    </div>
    <p class="hint">Chrome exports them itself: Password Manager › Settings › Export passwords. Castle seals each one and shreds the file.</p>
    <form v-if="adding" class="add" @submit.prevent="seal">
      <input v-model="form.url" placeholder="Site address" aria-label="Site address" />
      <input v-model="form.user" placeholder="Username" aria-label="Username" autocomplete="off" />
      <input v-model="form.pass" type="password" placeholder="Password" aria-label="Password" autocomplete="new-password" />
      <button class="primary" :disabled="busy || !form.url.trim() || !form.pass">Seal it</button>
    </form>
    <p v-if="said" class="said" role="status">{{ said }}</p>
    <ul class="list">
      <li v-for="l in shown" :key="l.id">
        <span class="globe" aria-hidden="true">◍</span>
        <span class="who"><b>{{ l.host }}</b><small>{{ l.username || '(no username)' }}</small></span>
        <span class="dots" aria-hidden="true">•••••••</span>
        <button :disabled="busy" title="Copy the password for 30 seconds" :aria-label="`Copy ${l.host}'s password`" @click="copy(l)">⧉</button>
        <button :disabled="busy" title="Delete" :aria-label="`Delete ${l.host}'s login`" @click="remove(l)">✕</button>
      </li>
      <li v-if="!shown.length" class="none">{{ logins.length ? 'No site matches.' : 'None yet. Import them from Chrome or add one.' }}</li>
    </ul>
    <footer>Sealed on this PC - no AI ever reads them.</footer>
  </aside>
</template>

<style scoped>
.sheet {
  position: absolute;
  z-index: 30;
  top: 54px;
  right: 14px;
  bottom: 14px;
  width: min(520px, calc(100% - 28px));
  display: flex;
  flex-direction: column;
  gap: 10px;
  padding: 16px 18px;
  border-radius: 16px;
  border: 1px solid rgba(143, 240, 203, 0.35);
  background: rgba(8, 14, 38, 0.97);
  box-shadow: 0 18px 50px rgba(0, 0, 0, 0.6);
  color: var(--text);
}
header {
  display: flex;
  align-items: center;
  gap: 10px;
}
.tag {
  font-size: 11px;
  letter-spacing: 0.2em;
  color: var(--text-dim);
}
h2 {
  flex: 1;
  margin: 0;
  font-size: 18px;
}
.close {
  border: 0;
  background: transparent;
  color: var(--text-dim);
  font-size: 20px;
  cursor: pointer;
}
.row,
.add {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
input {
  flex: 1;
  min-width: 120px;
  padding: 7px 10px;
  border-radius: 8px;
  border: 1px solid var(--line);
  background: rgba(5, 11, 31, 0.85);
  color: var(--text);
}
button {
  padding: 6px 12px;
  border-radius: 8px;
  border: 1px solid var(--line);
  background: rgba(150, 190, 255, 0.08);
  color: var(--text);
  cursor: pointer;
}
button:disabled {
  opacity: 0.5;
  cursor: default;
}
button.primary {
  border-color: rgba(143, 240, 203, 0.6);
  background: rgba(143, 240, 203, 0.18);
}
.hint,
.said {
  margin: 0;
  font-size: 12.5px;
  color: var(--text-dim);
}
.said {
  color: #bff5df;
}
.list {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  margin: 0;
  padding: 0;
  list-style: none;
}
.list li {
  display: flex;
  align-items: center;
  gap: 10px;
  padding: 8px 4px;
  border-bottom: 1px solid rgba(150, 190, 255, 0.1);
}
.who {
  flex: 1;
  min-width: 0;
  display: flex;
  flex-direction: column;
}
.who b,
.who small {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}
.who small {
  color: var(--text-dim);
}
.dots {
  color: var(--text-dim);
  letter-spacing: 0.1em;
}
.none {
  color: var(--text-dim);
  justify-content: center;
}
footer {
  font-size: 12px;
  color: var(--text-dim);
  text-align: center;
}
</style>
