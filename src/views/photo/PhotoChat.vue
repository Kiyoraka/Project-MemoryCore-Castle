<script setup>
// PHOTO's chat bar: the companion's portrait, the member's line, the send button,
// and the companion's last few words above it.
import { computed, ref, watch } from 'vue'

const props = defineProps({
  busy: { type: Boolean, default: false },
  lines: { type: Array, default: () => [] },
  portrait: { type: String, default: null },
  companion: { type: String, default: 'Your companion' },
})
const emit = defineEmits(['send'])

const SUGGESTIONS = ['Auto enhance', 'Make it warmer', 'Remove the background']

const text = ref('')
const portraitBroken = ref(false)

watch(() => props.portrait, () => (portraitBroken.value = false))

// the last three, oldest first
const trail = computed(() => props.lines.filter((l) => typeof l === 'string' && l.trim()).slice(-3))
const canSend = computed(() => !props.busy && text.value.trim().length > 0)

function send() {
  const message = text.value.trim()
  if (!message || props.busy) return
  emit('send', message)
  text.value = ''
}

function suggest(words) {
  if (props.busy) return
  text.value = words
  send()
}
</script>

<template>
  <div class="photo-chat">
    <div v-if="trail.length" class="trail">
      <div
        v-for="(line, i) in trail"
        :key="`${lines.length}-${i}`"
        class="bubble"
        :style="{ opacity: 1 - (trail.length - 1 - i) * 0.3 }"
      >{{ line }}</div>
    </div>
    <div v-if="!busy && !text" class="chips">
      <button v-for="s in SUGGESTIONS" :key="s" type="button" class="chip" @click="suggest(s)">{{ s }}</button>
    </div>

    <div class="bar">
      <img v-if="portrait && !portraitBroken" class="portrait" :src="portrait" :alt="companion" @error="portraitBroken = true" />
      <div v-else class="portrait fallback" :aria-label="companion">{{ (companion || '?').trim().charAt(0).toUpperCase() }}</div>

      <div class="field">
        <input
          v-model="text"
          type="text"
          :disabled="busy"
          :placeholder="busy ? '' : `Tell ${companion} what to do with the photo`"
          @keydown.enter.prevent="send"
        />
        <div v-if="busy" class="working">
          {{ companion }} is working<span class="dots"><i>.</i><i>.</i><i>.</i></span>
        </div>
        <button type="button" class="send" :disabled="!canSend" title="Send" aria-label="Send" @click="send">
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3.4 20.4 21 12 3.4 3.6l2.3 7.2L15 12l-9.3 1.2z" /></svg>
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.photo-chat {
  display: flex;
  flex-direction: column;
  gap: 8px;
  color: var(--text, #efe8ff);
}

.trail {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 6px;
  padding-left: 120px;
}

.bubble {
  max-width: min(640px, 90%);
  padding: 7px 14px;
  border: 1px solid var(--line, rgba(190, 170, 255, 0.3));
  border-radius: 14px 14px 14px 4px;
  background: var(--panel-background, rgba(24, 19, 66, 0.72));
  backdrop-filter: blur(var(--glass-blur, 8px));
  font-size: 13px;
  line-height: 1.4;
  transition: opacity 0.3s;
}

.chips {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  padding-left: 120px;
}

.chip {
  padding: 5px 14px;
  border: 1px solid var(--line, rgba(190, 170, 255, 0.3));
  border-radius: 999px;
  background: color-mix(in srgb, var(--accent, #8a5cf0) 14%, transparent);
  color: var(--text-dim, #c9bdf0);
  font-size: 13px;
  cursor: pointer;
}

.chip:hover {
  border-color: var(--accent, #8a5cf0);
  color: var(--text, #efe8ff);
}

.bar {
  display: flex;
  align-items: center;
  gap: 16px;
  padding: 12px 14px;
  border: 1px solid var(--line, rgba(190, 170, 255, 0.3));
  border-radius: 18px;
  background: var(--panel-background, rgba(24, 19, 66, 0.72));
  backdrop-filter: blur(var(--glass-blur, 8px));
}

.portrait {
  flex: 0 0 auto;
  width: 92px;
  height: 92px;
  border-radius: 50%;
  object-fit: cover;
  border: 2px solid color-mix(in srgb, var(--accent, #8a5cf0) 60%, transparent);
}

.portrait.fallback {
  display: grid;
  place-items: center;
  background: color-mix(in srgb, var(--accent, #8a5cf0) 35%, transparent);
  color: #fff;
  font-size: 34px;
  font-weight: 600;
}

.field {
  position: relative;
  display: flex;
  flex: 1;
  align-items: center;
  min-width: 0;
  height: 92px;
  padding: 0 14px 0 32px;
  border: 1px solid var(--line, rgba(190, 170, 255, 0.3));
  border-radius: 999px;
  background: rgba(255, 255, 255, 0.03);
}

.field:focus-within {
  border-color: var(--accent, #8a5cf0);
}

input {
  flex: 1;
  min-width: 0;
  border: none;
  outline: none;
  background: transparent;
  color: var(--text, #efe8ff);
  font: inherit;
  font-size: 18px;
}

input::placeholder {
  color: var(--text-dim, #c9bdf0);
  opacity: 0.7;
}

.working {
  position: absolute;
  left: 32px;
  color: var(--text-dim, #c9bdf0);
  font-size: 16px;
  pointer-events: none;
}

.dots i {
  font-style: normal;
  animation: blink 1.2s infinite;
}

.dots i:nth-child(2) {
  animation-delay: 0.2s;
}

.dots i:nth-child(3) {
  animation-delay: 0.4s;
}

@keyframes blink {
  0%, 80%, 100% { opacity: 0.2; }
  40% { opacity: 1; }
}

.send {
  display: grid;
  flex: 0 0 auto;
  place-items: center;
  width: 70px;
  height: 70px;
  padding: 0;
  border: none;
  border-radius: 50%;
  background: var(--accent, #8a5cf0);
  color: #fff;
  cursor: pointer;
  box-shadow: 0 0 16px color-mix(in srgb, var(--accent, #8a5cf0) 55%, transparent);
}

.send:disabled {
  opacity: 0.45;
  cursor: default;
  box-shadow: none;
}

.send svg {
  width: 30px;
  height: 30px;
  fill: currentColor;
}

@media (max-width: 720px) {
  .trail,
  .chips {
    padding-left: 0;
  }

  .portrait,
  .field {
    width: 56px;
    height: 56px;
  }

  .field {
    width: auto;
    padding-left: 18px;
  }

  .send {
    width: 44px;
    height: 44px;
  }
}
</style>
