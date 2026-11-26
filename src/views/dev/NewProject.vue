<script setup>
import GlassSelect from '../../components/GlassSelect.vue'
import { computed, onMounted, ref, watch } from 'vue'
import ModalSheet from '../../components/ModalSheet.vue'
import { backupSignIn, chooseFolder, openUrl } from '../../api/castle.js'
import { devClone, devCreate, devGithub, devKinds, devRoot, devSetRoot } from '../../api/devnew.js'
import { canSubmit, githubLine, landingFolder, nameFromLink, OTHER, setupSentence, STACK, stackPreview, validRepoName } from './newproject.js'

// New project, from the DEV room's left column: a repo cloned from a link, or a new repo made on
// GitHub, either way into the projects folder (<root>\<kind>\<name>). The name and the owner are
// the member's, never guessed beyond the link's own name. `made` gives back the new folder.
defineProps({ companion: { type: String, default: 'Your companion' } })
const emit = defineEmits(['made', 'close'])

const root = ref(null)
// true while the folder is only Castle's suggestion (<user folder>\Projects), made on the first project
const suggested = ref(false)
const rootRead = ref(false)
const kinds = ref([])
const kind = ref('')
// the stack the member's companion is asked to set up (optional: none by default)
const picks = ref(Object.fromEntries(STACK.map((s) => [s.id, ''])))
const others = ref(Object.fromEntries(STACK.map((s) => [s.id, ''])))
const stackOptions = (s) => [{ value: '', label: 'none' }, ...s.options, { value: OTHER, label: 'Other...' }]
const preview = computed(() => stackPreview(picks.value, others.value))
const mode = ref('link')
const link = ref('')
const name = ref('')
const owner = ref('')
const isPrivate = ref(true)
const github = ref(null)
const working = ref(false)
const checking = ref(false)
const problem = ref('')
let nameTouched = false
// once the member types a name, a pasted link no longer changes it (an emptied box listens again)
const touchName = () => (nameTouched = name.value.trim() !== '')

watch(link, (text) => {
  if (!nameTouched) name.value = nameFromLink(text)
})

const line = computed(() => githubLine(github.value))
const owners = computed(() => github.value?.owners ?? [])
const nameBad = computed(() => name.value.trim() !== '' && !validRepoName(name.value.trim()))
const landing = computed(() => landingFolder(root.value, kind.value, name.value))
const ready = computed(() =>
  canSubmit({ mode: mode.value, root: root.value, link: link.value, name: name.value, owner: owner.value, github: github.value, working: working.value }),
)

async function readKinds() {
  try {
    kinds.value = (await devKinds()) ?? []
  } catch {
    kinds.value = []
  }
  if (kind.value && !kinds.value.includes(kind.value)) kind.value = ''
}

async function readGithub() {
  checking.value = true
  try {
    github.value = await devGithub()
  } catch {
    github.value = { installed: false, signedIn: false, login: null, owners: [] }
  } finally {
    checking.value = false
  }
  if (!owners.value.includes(owner.value)) owner.value = owners.value[0] ?? ''
}

async function changeRoot() {
  problem.value = ''
  try {
    const folder = await chooseFolder('Where your projects live')
    if (!folder) return
    await devSetRoot(folder)
    root.value = folder
    suggested.value = false
    await readKinds()
  } catch (reason) {
    problem.value = String(reason)
  }
}

async function signIn() {
  problem.value = ''
  try {
    await backupSignIn()
  } catch (reason) {
    problem.value = String(reason)
  }
}

async function make() {
  if (!ready.value) return
  problem.value = ''
  working.value = true
  try {
    const folder =
      mode.value === 'link'
        ? await devClone(link.value.trim(), kind.value, name.value.trim())
        : await devCreate(owner.value, name.value.trim(), isPrivate.value, kind.value)
    suggested.value = false
    emit('made', folder, setupSentence(picks.value, others.value))
  } catch (reason) {
    problem.value = String(reason)
  } finally {
    working.value = false
  }
}

onMounted(async () => {
  try {
    const place = (await devRoot()) ?? {}
    root.value = place.folder || null
    suggested.value = Boolean(place.suggested)
  } catch (reason) {
    problem.value = String(reason)
  }
  rootRead.value = true
  if (root.value) readKinds()
  readGithub()
})
</script>

<template>
  <ModalSheet tag="DEV" title="New project" @close="working || emit('close')">
    <div class="new-project">
      <section class="root" aria-label="Projects folder">
        <h4>Projects folder</h4>
        <p v-if="!rootRead" class="quiet">Reading…</p>
        <template v-else-if="root">
          <div class="row">
            <code class="path" :title="root">{{ root }}</code>
            <span v-if="suggested" class="tag" title="Castle makes this folder with your first project. Change it any time.">suggested</span>
            <button class="small" :disabled="working" @click="changeRoot">Change…</button>
          </div>
        </template>
        <template v-else>
          <p class="quiet">Choose the folder your projects live in. New projects are made inside it.</p>
          <div class="row">
            <button class="small primary" @click="changeRoot">Choose a folder…</button>
          </div>
        </template>
      </section>

      <template v-if="root">
        <div class="ways" role="tablist" aria-label="How to make it">
          <button role="tab" :aria-selected="mode === 'link'" :class="{ on: mode === 'link' }" :disabled="working" @click="mode = 'link'">Clone from a link</button>
          <button role="tab" :aria-selected="mode === 'create'" :class="{ on: mode === 'create' }" :disabled="working" @click="mode = 'create'">Create on GitHub</button>
        </div>

        <div class="form" role="tabpanel">
          <label v-if="mode === 'link'" class="field">
            <span>Link</span>
            <input v-model="link" placeholder="https://github.com/owner/repo.git" autocomplete="off" spellcheck="false" :disabled="working" />
          </label>

          <template v-else>
            <p class="gh" :class="line">
              <template v-if="line === 'asking'">Asking GitHub…</template>
              <template v-else-if="line === 'install'">
                The GitHub CLI is not on this computer.
                <button class="link" @click="openUrl('https://cli.github.com')">Install the GitHub CLI</button>
                <button class="small" :disabled="checking" @click="readGithub">{{ checking ? 'Checking…' : 'Check again' }}</button>
              </template>
              <template v-else-if="line === 'signin'">
                Not signed in to GitHub.
                <button class="small primary" @click="signIn">Sign in to GitHub</button>
                <button class="small" :disabled="checking" @click="readGithub">{{ checking ? 'Checking…' : 'Check again' }}</button>
              </template>
              <template v-else>Signed in as {{ github.login }}</template>
            </p>
            <label class="field">
              <span>Owner</span>
              <GlassSelect v-model="owner" :options="owners" :disabled="working || line !== 'ready' || !owners.length" aria-label="Owner" />
            </label>
          </template>

          <label class="field">
            <span>Name</span>
            <input v-model="name" placeholder="order-tracker" autocomplete="off" spellcheck="false" :disabled="working" :aria-invalid="nameBad" @input="touchName" />
          </label>
          <p v-if="nameBad" class="hint bad">Letters, digits, "-", "_" and "." only, and not starting with ".".</p>

          <label v-if="mode === 'create'" class="check">
            <input v-model="isPrivate" type="checkbox" :disabled="working" />
            Private
          </label>

          <label class="field">
            <span>Kind</span>
            <GlassSelect v-model="kind" :options="[{ value: '', label: '(none - straight in the projects folder)' }, ...kinds]" :disabled="working" aria-label="Kind" />
          </label>

          <p class="landing"><span>Folder</span><code>{{ landing }}</code></p>

          <fieldset class="stack">
            <legend>Stack (optional)</legend>
            <div class="stack-grid">
              <label v-for="s in STACK" :key="s.id" class="field">
                <span>{{ s.label }}</span>
                <GlassSelect v-model="picks[s.id]" :options="stackOptions(s)" :disabled="working" :aria-label="s.label" />
                <input v-if="picks[s.id] === OTHER" v-model="others[s.id]" maxlength="60" :placeholder="`Your ${s.label.toLowerCase()}`" :disabled="working" :aria-label="`Other ${s.label.toLowerCase()}`" />
              </label>
            </div>
            <p class="hint">{{ preview ? `${companion} will set up: ${preview}` : 'Leave it all on none to start with an empty project.' }}</p>
          </fieldset>
        </div>

        <p v-if="problem" class="problem" role="alert">{{ problem }}</p>
        <div class="buttons">
          <button class="small" :disabled="working" @click="emit('close')">Cancel</button>
          <button class="small primary" :disabled="!ready" @click="make">
            {{ working ? (mode === 'link' ? 'Cloning…' : 'Creating…') : mode === 'link' ? 'Clone and open' : 'Create and open' }}
          </button>
        </div>
      </template>
      <p v-else-if="problem" class="problem" role="alert">{{ problem }}</p>
    </div>
  </ModalSheet>
</template>

<style scoped>
.new-project {
  display: flex;
  flex-direction: column;
  gap: 14px;
  max-width: 640px;
  margin: 0 auto;
  font-size: 13px;
}

h4 {
  margin: 0 0 6px;
  font-size: 11px;
  font-weight: 600;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--text-dim);
}

.row {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
}

.path {
  flex: 1;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

code {
  font-family: 'Cascadia Mono', Consolas, ui-monospace, Menlo, monospace;
  font-size: 12px;
  overflow-wrap: anywhere;
}

.tag {
  padding: 1px 8px;
  border: 1px solid color-mix(in srgb, var(--accent) 60%, transparent);
  border-radius: 999px;
  color: var(--accent);
  font-size: 11px;
}

.quiet {
  margin: 0 0 6px;
  color: var(--text-dim);
}

.ways {
  display: flex;
  gap: 6px;
  border-bottom: 1px solid var(--line);
}

.ways button {
  padding: 7px 14px;
  border: 0;
  border-bottom: 2px solid transparent;
  border-radius: 8px 8px 0 0;
  background: none;
  color: var(--text-dim);
  font: inherit;
  cursor: pointer;
}

.ways button:hover:not(:disabled) {
  background: var(--hover);
}

.ways button.on {
  color: var(--text);
  border-bottom-color: var(--accent);
}

.form {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.field,
.landing {
  display: flex;
  align-items: center;
  gap: 10px;
  margin: 0;
}

.field > span,
.landing > span {
  width: 60px;
  flex: none;
  color: var(--text-dim);
}

/* text boxes in the theme's own colours (the dropdowns beside them are GlassSelect) */
.field input {
  flex: 1;
  min-width: 0;
  box-sizing: border-box;
  padding: 7px 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--widget-background);
  color: var(--text);
  font: inherit;
  font-size: 13px;
}

.field input::placeholder {
  color: var(--text-dim);
  opacity: 0.7;
}

.field > *:not(span) {
  flex: 1;
  min-width: 0;
}

.field input[aria-invalid='true'] {
  border-color: var(--warn);
}

.field input:focus-visible {
  outline: none;
  border-color: var(--accent);
}

.check {
  display: flex;
  align-items: center;
  gap: 8px;
  margin-left: 70px;
}

.check input {
  margin: 0;
  accent-color: var(--accent);
}

.stack {
  display: grid;
  gap: 8px;
  margin: 0;
  padding: 10px 12px 12px;
  border: 1px solid var(--line);
  border-radius: 10px;
}
.stack legend {
  padding: 0 6px;
  font-size: 12px;
  color: var(--text-dim);
}
.stack-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
  gap: 12px;
}

/* in the stack grid the label sits above its dropdown, so narrow columns stay tidy */
.stack-grid .field {
  flex-direction: column;
  align-items: stretch;
  gap: 4px;
}

.stack-grid .field > span {
  width: auto;
  font-size: 12px;
}
.hint {
  margin: -4px 0 0 70px;
  font-size: 12px;
  color: var(--text-dim);
}

.hint.bad {
  color: var(--warn);
}

.gh {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  margin: 0;
  padding: 8px 10px;
  border: 1px solid var(--line);
  border-radius: 10px;
  background: var(--hover);
}

.gh.ready {
  border-color: color-mix(in srgb, var(--good) 50%, transparent);
  color: var(--good);
}

.problem {
  margin: 0;
  color: var(--warn);
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}

.buttons {
  display: flex;
  justify-content: flex-end;
  gap: 8px;
}

.small {
  padding: 5px 12px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}

.small.primary {
  border-color: var(--accent);
  background: var(--accent);
  color: #07142e;
  font-weight: 600;
}

.small:hover:not(:disabled) {
  filter: brightness(1.12);
  background-color: var(--hover);
}

.small.primary:hover:not(:disabled) {
  background-color: var(--accent);
}

.small:disabled,
.ways button:disabled {
  opacity: 0.45;
  cursor: not-allowed;
}

.link {
  padding: 0;
  border: 0;
  background: none;
  color: var(--accent);
  font: inherit;
  cursor: pointer;
}

.link:hover {
  text-decoration: underline;
}

.small:focus-visible,
.link:focus-visible,
.ways button:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}
</style>
