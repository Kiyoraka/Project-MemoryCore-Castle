<script setup>
// One line of the chat room: a system note, the member's own bubble (on the right, with its
// ticks), or someone else's (with a face). A reply shows its quote; reactions show as chips and a
// small bar on hover or focus, only when the room offers them; a + opens a picker for any one emoji
// when the room offers customReactions. Props in, events out.
import { ref } from 'vue'
import { REACTIONS, reactionChips, replySnippet } from '../roomchat.js'
import { linkSpans } from '../markdown.js'
import { linkWay, openLink } from '../links.js'
import EmojiPicker from './EmojiPicker.vue'
import ThemedAudio from '../ThemedAudio.vue'

const props = defineProps({
  line: { type: Object, required: true },
  mine: { type: Boolean, default: false },
  // every kept line, so a reply's quote knows whether its original is loaded
  lines: { type: Array, default: () => [] },
  // { name, title } of the speaker (speakerPresentation)
  speaker: { type: Object, default: () => ({ name: '', title: '' }) },
  ai: { type: Boolean, default: false },
  face: { type: String, default: null },
  initial: { type: String, default: '' },
  // { [fileId]: url } for voice lines and pictures
  files: { type: Object, default: () => ({}) },
  clock: { type: String, default: '' },
  // the member's ticks: { mark, read, title }
  tick: { type: Object, default: null },
  // what the room offers: { replies, reactions, customReactions }
  can: { type: Object, default: () => ({ replies: false, reactions: false, customReactions: false }) },
})
const emit = defineEmits(['react', 'reply', 'jump', 'open-picture'])

const quote = () => replySnippet(props.line, props.lines)
const chips = () => reactionChips(props.line)
// the picker is open on this line (the bar stays shown while it is)
const picking = ref(false)
function picked(emoji) {
  picking.value = false
  emit('react', emoji)
}
</script>

<template>
  <p v-if="line.author?.kind === 'system'" class="system"><em>{{ line.text }}</em></p>
  <div v-else-if="mine" :id="'room-line-' + line.id" class="bubble mine" :class="{ picking }" tabindex="-1">
    <button v-if="quote()" class="quote" :disabled="!quote().loaded" @click="emit('jump', line.replyTo.id)"><strong>{{ quote().name }}</strong> {{ quote().text }}</button>
    <span class="text"><template v-for="(span, s) in linkSpans(line.text)" :key="s"><a v-if="linkWay(span.link)" class="link" :href="span.link" :title="span.link" @click.prevent="openLink(span.link).catch(() => {})">{{ span.text }}</a><template v-else>{{ span.text }}</template></template></span>
    <span class="meta">{{ clock }} <span v-if="tick" class="tick" :class="{ read: tick.read }" :title="tick.title">{{ tick.mark }}</span></span>
    <span v-if="chips().length" class="chips">
      <button v-for="r in chips()" :key="r.emoji" class="chip" :class="{ mine: r.mine }" :disabled="!can.reactions" @click="emit('react', r.emoji)">{{ r.emoji }} {{ r.count }}</button>
    </span>
    <span v-if="can.reactions || can.replies" class="tools" role="toolbar" aria-label="React or reply">
      <template v-if="can.reactions"><button v-for="e in REACTIONS" :key="e" :aria-label="'React ' + e" @click="emit('react', e)">{{ e }}</button></template>
      <button v-if="can.reactions && can.customReactions" class="more" aria-label="Another reaction" title="Another reaction" :aria-expanded="picking" @click="picking = !picking">+</button>
      <button v-if="can.replies" @click="emit('reply')">Reply</button>
    </span>
    <EmojiPicker v-if="picking" class="pick-mine" @pick="picked" @close="picking = false" />
  </div>
  <div v-else :id="'room-line-' + line.id" class="bubble theirs" :class="[line.author?.kind, { ai, picking }]" tabindex="-1">
    <span class="face"><img v-if="face" :src="face" alt="" /><template v-else>{{ initial }}</template></span>
    <div class="body">
      <span class="who" :title="speaker.title" :tabindex="ai ? 0 : undefined" :aria-label="speaker.title ? `${speaker.name}, ${speaker.title}` : undefined">{{ speaker.name }}<span v-if="ai" class="spark"> ✦</span></span>
      <button v-if="quote()" class="quote" :disabled="!quote().loaded" @click="emit('jump', line.replyTo.id)"><strong>{{ quote().name }}</strong> {{ quote().text }}</button>
      <span class="text"><template v-for="(span, s) in linkSpans(line.text)" :key="s"><a v-if="linkWay(span.link)" class="link" :href="span.link" :title="span.link" @click.prevent="openLink(span.link).catch(() => {})">{{ span.text }}</a><template v-else>{{ span.text }}</template></template></span>
      <ThemedAudio v-if="line.voice && files[line.voice.fileId]" class="voice" :src="files[line.voice.fileId]" compact />
      <img
        v-if="line.picture && files[line.picture.fileId]"
        :src="files[line.picture.fileId]"
        alt="A picture in the room"
        class="picture"
        role="button"
        tabindex="0"
        title="Open the picture"
        @click="emit('open-picture', line.picture.fileId)"
        @keydown.enter.prevent="emit('open-picture', line.picture.fileId)"
      />
      <!-- inside the message box, so the reactions and the bar follow the box wherever it sits -->
      <span v-if="chips().length" class="chips">
        <button v-for="r in chips()" :key="r.emoji" class="chip" :class="{ mine: r.mine }" :disabled="!can.reactions" @click="emit('react', r.emoji)">{{ r.emoji }} {{ r.count }}</button>
      </span>
      <span v-if="can.reactions || can.replies" class="tools" role="toolbar" aria-label="React or reply">
        <template v-if="can.reactions"><button v-for="e in REACTIONS" :key="e" :aria-label="'React ' + e" @click="emit('react', e)">{{ e }}</button></template>
      <button v-if="can.reactions && can.customReactions" class="more" aria-label="Another reaction" title="Another reaction" :aria-expanded="picking" @click="picking = !picking">+</button>
        <button v-if="can.replies" @click="emit('reply')">Reply</button>
      </span>
      <EmojiPicker v-if="picking" class="pick-theirs" @pick="picked" @close="picking = false" />
    </div>
    <span class="meta">{{ clock }}</span>
  </div>
</template>

<style scoped>
.bubble {
  position: relative;
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
  position: relative;
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
.who {
  color: var(--text-dim);
}
.text {
  white-space: pre-wrap;
  overflow-wrap: anywhere;
}
.body .voice {
  max-width: 320px;
  margin-top: 4px;
}
.body .picture {
  max-width: 320px;
  border-radius: 8px;
  cursor: zoom-in;
}
.quote {
  display: block;
  max-width: 100%;
  margin-bottom: 4px;
  padding: 4px 8px;
  overflow: hidden;
  border: 0;
  border-left: 3px solid var(--accent);
  border-radius: 6px;
  background: color-mix(in srgb, var(--accent) 12%, transparent);
  color: var(--text-dim);
  font: inherit;
  font-size: 12px;
  text-align: left;
  text-overflow: ellipsis;
  white-space: nowrap;
  cursor: pointer;
}
.quote:disabled {
  cursor: default;
}
.quote strong {
  margin-right: 4px;
  color: var(--text);
}
.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 4px;
  margin-top: 4px;
}
.chip {
  padding: 1px 8px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: rgba(0, 0, 0, 0.2);
  color: var(--text);
  font: inherit;
  font-size: 12px;
  cursor: pointer;
}
.chip.mine {
  border-color: var(--accent);
  background: color-mix(in srgb, var(--accent) 22%, transparent);
}
.tools {
  position: absolute;
  top: -14px;
  right: 8px;
  display: none;
  gap: 2px;
  padding: 2px 4px;
  border: 1px solid var(--line);
  border-radius: 999px;
  background: color-mix(in srgb, var(--panel-background) 92%, #050b1f);
  box-shadow: 0 6px 18px rgba(0, 0, 0, 0.35);
}
/* The bar hangs from the message box: from its left corner for someone else's line and its right
   corner for the member's own, so a bar wider than a short message always grows into the empty space
   beside it and never past the edge of the list. */
.bubble.theirs .tools {
  right: auto;
  left: 8px;
}
/* the bar shows when the pointer is on the message box itself (not the empty space beside it) or
   when something in the line has the keyboard */
.bubble.mine:hover .tools,
.bubble.theirs .body:hover .tools,
.bubble:focus-within .tools,
.bubble.picking .tools {
  display: flex;
}
/* the picker opens under the bar, from the same corner the bar hangs from */
.pick-mine {
  right: 8px;
}
.pick-theirs {
  left: 8px;
}
.bubble.picking {
  z-index: 4;
}
.tools button {
  padding: 2px 6px;
  border: 0;
  border-radius: 999px;
  background: none;
  color: var(--text);
  font: inherit;
  font-size: 13px;
  cursor: pointer;
}
.tools button:hover,
.tools button:focus-visible {
  background: var(--hover);
}
.text a.link {
  color: var(--accent);
  text-decoration: underline;
  cursor: pointer;
  overflow-wrap: anywhere;
}
</style>
