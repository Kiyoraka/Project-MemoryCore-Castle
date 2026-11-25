<script setup>
// One pane of the browser, the Keep's way: its own toolbar (back, forward, reload, the address with
// its own suggestions, star, Fill, translate, Put in chat, Open in your browser, the column's x),
// the site's warning as a banner, its translate row, then the box its page sits in. Castle's page
// measures that box and the core puts the page there (browser_show), so a pane's own rows push only
// its own page down.
import { computed, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import {
  browserBookmarkToggle,
  browserFill,
  browserForgetSearch,
  browserGo,
  browserIsBookmarked,
  browserLogins,
  browserOutside,
  browserPutInChat,
  browserStep,
  browserSuggest,
  browserTranslate,
  browserTranslateLanguage,
  browserTranslateOff,
  browserTranslatePictures,
  browserTranslateSite,
} from '../api/castle.js'
import SuggestList from './SuggestList.vue'
import TranslateRow from './TranslateRow.vue'
import { fillLabel, moveSelection, showTranslateRow, siteWarning, suggestionGroups, suggestionInput } from './browserview.js'

const props = defineProps({
  tab: { type: Object, required: true },
  focused: { type: Boolean, default: false },
  // more than one column on screen: the column's x shows
  many: { type: Boolean, default: false },
  translating: { type: Object, required: true },
})
const emit = defineEmits(['focus', 'close-column', 'overlay', 'page-el', 'problem', 'translating'])

const field = ref(null)
const pageBox = ref(null)
const typed = ref('')
const url = computed(() => props.tab.url ?? '')
const secure = computed(() => url.value.startsWith('https://'))
watch(url, (now) => (typed.value = now), { immediate: true })

async function act(work) {
  emit('problem', '')
  try {
    await work()
  } catch (reason) {
    emit('problem', String(reason))
  }
}

// ---------- the address and its suggestions ----------
const suggestions = ref([])
const selected = ref(-1)
const editing = ref(false)
const order = computed(() => suggestionGroups(suggestions.value).order)
const suggestOpen = computed(() => editing.value && suggestions.value.length > 0)
watch(suggestOpen, (open) => emit('overlay', `suggest-${props.tab.id}`, open))
let suggestTimer = null

async function askSuggest() {
  const asked = typed.value
  const rows = await browserSuggest(asked).catch(() => [])
  if (asked === typed.value && editing.value) suggestions.value = rows
}
function onType() {
  clearTimeout(suggestTimer)
  selected.value = -1
  suggestTimer = setTimeout(askSuggest, 120)
}
function closeSuggest() {
  clearTimeout(suggestTimer)
  suggestions.value = []
  selected.value = -1
}
function onFieldFocus(event) {
  editing.value = true
  emit('focus', props.tab.id)
  event.target.select()
  if (!typed.value.trim()) askSuggest()
}
function onFieldBlur() {
  editing.value = false
  closeSuggest()
}
function onFieldKey(event) {
  if (!suggestOpen.value) return
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    selected.value = moveSelection(selected.value, event.key, order.value.length)
    event.preventDefault()
  } else if (event.key === 'Escape') {
    closeSuggest()
    event.preventDefault()
  }
}
function pickSuggestion(row) {
  closeSuggest()
  field.value?.blur()
  act(() => browserGo(suggestionInput(row), props.tab.id))
}
function forgetSuggestion(row) {
  act(async () => {
    await browserForgetSearch(row.text)
    await askSuggest()
  })
}
function go() {
  const row = selected.value >= 0 ? order.value[selected.value] : null
  if (row) return pickSuggestion(row)
  closeSuggest()
  field.value?.blur()
  act(() => browserGo(typed.value, props.tab.id))
}
const step = (name) => act(() => browserStep(name, props.tab.id))
const putInChat = () => act(() => browserPutInChat(url.value))
const outside = () => act(() => browserOutside(url.value))
/** Ctrl+L: the address of this pane, selected. */
function selectAddress() {
  field.value?.focus()
  field.value?.select()
}

// ---------- the star and Fill (a password never reaches this page) ----------
const starred = ref(false)
const logins = ref([])
const choosing = ref(false)
async function readPage() {
  choosing.value = false
  if (!url.value) {
    starred.value = false
    logins.value = []
    return
  }
  starred.value = await browserIsBookmarked(url.value).catch(() => false)
  logins.value = await browserLogins(props.tab.id).catch(() => [])
}
watch(url, readPage, { immediate: true })
// a sign-in page loading in the same address: read again once the page has had a moment
watch(
  () => props.tab.title,
  () => url.value && browserLogins(props.tab.id).then((l) => (logins.value = l)).catch(() => {}),
)
const toggleStar = () => act(async () => (starred.value = await browserBookmarkToggle(url.value, props.tab.title ?? '')))
function fill(login) {
  choosing.value = false
  act(() => browserFill(login.id, props.tab.id))
}
const pressFill = () => (logins.value.length === 1 ? fill(logins.value[0]) : (choosing.value = !choosing.value))

// ---------- the site's warning, as a banner in the pane ----------
const warning = computed(() => (props.tab.warning?.verdict === 'safe' ? '' : siteWarning(props.tab.warning)))

// ---------- this pane's translate row ----------
const translateAsked = ref(false)
const translateBusy = ref('')
const translateSaid = ref('')
const bubbleCount = ref(0)
const translateShown = computed(() => showTranslateRow(props.tab, props.translating.language, translateAsked.value))
watch(url, () => {
  translateAsked.value = false
  translateSaid.value = ''
  bubbleCount.value = 0
})
async function translateNow(quiet = false) {
  if (translateBusy.value) return
  if (!quiet) translateBusy.value = 'Translating with your Claude Code...'
  try {
    const done = await browserTranslate(props.tab.id, props.translating.language)
    if (!quiet) translateSaid.value = done.same ? 'This page is already in your language.' : done.translated ? '' : 'Nothing left to translate on screen.'
  } catch (reason) {
    if (!quiet) translateSaid.value = String(reason)
  } finally {
    if (!quiet) translateBusy.value = ''
  }
}
const translateOriginal = () =>
  act(async () => {
    await browserTranslateOff(props.tab.id)
    bubbleCount.value = 0
  })
async function translateLanguage(code) {
  const language = await browserTranslateLanguage(code).catch(() => props.translating.language)
  emit('translating', { ...props.translating, language })
  if (props.tab.translated) {
    await browserTranslateOff(props.tab.id).catch(() => {})
    translateNow()
  }
}
const translateAlways = (on) =>
  act(async () => {
    emit('translating', { ...props.translating, sites: await browserTranslateSite(props.tab.id, on) })
    if (on && !props.tab.translated) translateNow()
  })
async function readBubbles() {
  if (translateBusy.value) return
  translateBusy.value = 'Reading the bubbles... (10 to 30 seconds)'
  try {
    bubbleCount.value += await browserTranslatePictures(props.tab.id, props.translating.language)
    translateSaid.value = bubbleCount.value ? '' : 'No bubbles to translate on screen.'
  } catch (reason) {
    translateSaid.value = String(reason)
  } finally {
    translateBusy.value = ''
  }
}
// while a page is read in another language, what scrolls into view is translated too
let rounds = null
watch(
  () => props.tab.translated,
  (on) => {
    clearInterval(rounds)
    if (on) rounds = setInterval(() => translateNow(true), 4000)
  },
  { immediate: true },
)

onMounted(() => emit('page-el', props.tab.id, pageBox.value))
onBeforeUnmount(() => {
  clearInterval(rounds)
  clearTimeout(suggestTimer)
  emit('overlay', `suggest-${props.tab.id}`, false)
  emit('overlay', `listing-${props.tab.id}`, false)
  emit('page-el', props.tab.id, null)
})
defineExpose({ selectAddress })
</script>

<template>
  <section class="pane" :class="{ focused: focused && many, private: tab.private }" @pointerdown="emit('focus', tab.id)">
    <div class="bar">
      <button class="nav" aria-label="Back" :disabled="!url" @click="step('back')">‹</button>
      <button class="nav" aria-label="Forward" :disabled="!url" @click="step('forward')">›</button>
      <button class="nav" aria-label="Reload" :disabled="!url" @click="step('reload')">↻</button>
      <form class="address" @submit.prevent="go">
        <svg width="13" height="13" viewBox="0 0 24 24" fill="none" :stroke="secure ? 'var(--good)' : 'var(--warn)'" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="5" y="11" width="14" height="10" rx="2" /><path d="M8 11V7a4 4 0 0 1 8 0v4" /></svg>
        <input
          ref="field"
          v-model="typed"
          :aria-label="`Address of ${tab.title}`"
          placeholder="Search or type an address"
          spellcheck="false"
          autocomplete="off"
          role="combobox"
          :aria-expanded="suggestOpen"
          @focus="onFieldFocus"
          @blur="onFieldBlur"
          @input="onType"
          @keydown="onFieldKey"
        />
        <SuggestList v-if="suggestOpen" class="suggest-under" :rows="suggestions" :selected="selected" @pick="pickSuggestion" @forget="forgetSuggestion" @hover="selected = $event" />
      </form>
      <button class="nav star" :class="{ lit: starred }" :disabled="!url" :aria-label="starred ? 'Remove bookmark' : 'Bookmark this page'" :title="starred ? 'Remove bookmark' : 'Bookmark this page'" @click="toggleStar">{{ starred ? '★' : '☆' }}</button>
      <button v-if="url && logins.length" class="fill" title="Castle fills this page's sign-in from your vault" @click="pressFill">{{ fillLabel(logins) }}</button>
      <button class="nav" :disabled="!url" :class="{ lit: translateShown }" aria-label="Translate this page" title="Translate this page into your language" @click="translateAsked = !translateAsked">文A</button>
      <slot name="tools" :tab="tab" />
      <button class="nav" :disabled="!url" aria-label="Put in chat" title="Put this page's address in HOME's chat" @click="putInChat">
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M21 12a8 8 0 0 1-11.6 7.1L4 20l1-4.6A8 8 0 1 1 21 12z" /></svg>
      </button>
      <button class="nav" :disabled="!url" aria-label="Open in your browser" title="Open in your browser" @click="outside">
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M14 4h6v6" /><path d="M20 4l-9 9" /><path d="M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5" /></svg>
      </button>
      <button v-if="many" class="nav" aria-label="Take this column away" title="Take this column away (the tab stays open)" @click.stop="emit('close-column', tab.id)">×</button>
    </div>
    <div v-if="choosing" class="logins" role="menu">
      <button v-for="l in logins" :key="l.id" role="menuitem" @click="fill(l)">{{ l.username || '(no username)' }}</button>
      <button class="quiet" @click="choosing = false">Not now</button>
    </div>
    <div v-if="warning && !tab.danger" class="guard" :class="tab.warning?.verdict === 'dangerous' ? 'coral' : 'amber'" role="alert"><span class="sign">!</span>{{ warning }}</div>
    <TranslateRow
      v-if="translateShown"
      :tab="tab"
      :settings="translating"
      :busy="translateBusy"
      :said="translateSaid"
      :bubbles="bubbleCount"
      @translate="translateNow()"
      @original="translateOriginal"
      @language="translateLanguage"
      @always="translateAlways"
      @bubbles="readBubbles"
      @listing="emit('overlay', `listing-${tab.id}`, $event)"
    />
    <slot name="below" :tab="tab" />
    <div ref="pageBox" class="page">
      <slot name="page" :tab="tab" />
    </div>
  </section>
</template>

<style scoped>
.pane {
  min-width: 0;
  min-height: 0;
  display: flex;
  flex-direction: column;
  border: 1px solid var(--line);
  border-radius: 12px;
  overflow: hidden;
  background: rgba(10, 14, 40, 0.55);
}
.pane.focused {
  border-color: rgba(168, 140, 255, 0.9);
  box-shadow: 0 0 12px rgba(168, 140, 255, 0.25);
}
.pane.private {
  border-color: rgba(168, 120, 255, 0.75);
  background: rgba(70, 40, 140, 0.35);
}
.bar {
  position: relative;
  display: flex;
  align-items: center;
  gap: 6px;
  padding: 8px 10px;
}
.nav {
  flex-shrink: 0;
  min-width: 30px;
  height: 30px;
  padding: 0 6px;
  display: grid;
  place-items: center;
  border: 0;
  border-radius: 8px;
  background: transparent;
  color: var(--text-dim);
  font-size: 17px;
  cursor: pointer;
}
.nav:hover:not(:disabled) {
  background: var(--hover);
  color: var(--text);
}
.nav:disabled {
  opacity: 0.35;
  cursor: default;
}
.nav.lit {
  color: #f5c864;
}
.nav.star {
  font-size: 18px;
}
.address {
  position: relative;
  flex: 1;
  min-width: 80px;
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 7px 12px;
  border-radius: 9px;
  border: 1px solid var(--line);
  background: rgba(5, 11, 31, 0.85);
}
.address:focus-within {
  border-color: rgba(168, 140, 255, 0.9);
}
.address input {
  flex: 1;
  min-width: 0;
  border: 0;
  outline: none;
  background: transparent;
  color: var(--text);
  font-size: 13.5px;
}
.suggest-under {
  top: calc(100% + 6px);
  left: 0;
  right: 0;
}
.fill {
  flex-shrink: 0;
  padding: 5px 12px;
  border-radius: 999px;
  border: 1px solid rgba(126, 224, 184, 0.6);
  background: rgba(126, 224, 184, 0.14);
  color: #bff5df;
  font-size: 12.5px;
  cursor: pointer;
}
.logins {
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  padding: 0 10px 8px;
}
.logins button {
  padding: 4px 10px;
  border-radius: 8px;
  border: 1px solid var(--line);
  background: rgba(5, 11, 31, 0.85);
  color: var(--text);
  font-size: 12.5px;
  cursor: pointer;
}
.logins .quiet {
  color: var(--text-dim);
}
/* the site's warning, inside the pane (coral dangerous, amber suspicious) */
.guard {
  display: flex;
  align-items: center;
  gap: 8px;
  margin: 0 10px 8px;
  padding: 7px 12px;
  border-radius: 8px;
  font-size: 13px;
}
.guard.coral {
  background: rgba(230, 90, 110, 0.85);
  color: #fff;
}
.guard.amber {
  background: rgba(245, 180, 82, 0.85);
  color: #2a1a00;
}
.guard .sign {
  flex-shrink: 0;
  width: 18px;
  height: 18px;
  display: grid;
  place-items: center;
  border-radius: 50%;
  background: rgba(255, 255, 255, 0.85);
  color: #a3283c;
  font-weight: 800;
  font-size: 12px;
}
.page {
  position: relative;
  flex: 1;
  min-height: 0;
  display: flex;
}
</style>
