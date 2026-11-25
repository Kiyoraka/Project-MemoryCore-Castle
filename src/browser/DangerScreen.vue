<script setup>
// The DANGER screen (browser_guard.rs): drawn by Castle's own page where a held-back site would
// have been, so the site can never cover it or click past it. Red in every theme: a warning is not
// themed. Go back to safety is the bright way out; Open anyway lets one page of the site load.
import { computed } from 'vue'
import { isMac } from '../platform.js'
import { dangerWords } from './browserview.js'

const props = defineProps({ danger: { type: Object, required: true } })
const emit = defineEmits(['back', 'open'])
const words = computed(() => dangerWords(props.danger, isMac))
const tape = Array.from({ length: 6 }, (_, i) => i)
</script>

<template>
  <main class="danger" role="alertdialog" aria-labelledby="danger-title" aria-describedby="danger-threat">
    <div class="tape top" aria-hidden="true">
      <span v-for="i in tape" :key="i">WARNING <b>//</b></span>
    </div>
    <div class="middle">
      <svg class="sign" width="96" height="84" viewBox="0 0 24 21" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linejoin="round" aria-hidden="true">
        <path d="M12 1.5L23 20H1z" />
        <path d="M12 8v5.5" stroke-linecap="round" />
        <circle cx="12" cy="16.6" r="0.6" fill="currentColor" />
      </svg>
      <h1 id="danger-title">DANGER</h1>
      <p id="danger-threat" class="threat">{{ words.threat }}</p>
      <p class="reason">{{ words.reason }}</p>
      <p class="address" :title="words.address">{{ words.address }}</p>
      <ul class="chips" aria-label="Who caught it">
        <li v-for="c in words.chips" :key="c.id" :class="{ lit: c.lit }">
          <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" aria-hidden="true"><path d="M12 3l8 3v6c0 4.5-3.4 8.4-8 9-4.6-.6-8-4.5-8-9V6z" /></svg>
          {{ c.name }}<span class="hidden">{{ c.lit ? ' caught it' : '' }}</span>
        </li>
      </ul>
      <div class="ways">
        <button class="back" autofocus @click="emit('back')">Go back to safety</button>
        <button class="anyway" title="Opens this one page; the next visit is held back again" @click="emit('open')">Open anyway</button>
      </div>
    </div>
    <div class="tape bottom" aria-hidden="true">
      <span v-for="i in tape" :key="i">WARNING <b>//</b></span>
    </div>
  </main>
</template>

<style scoped>
.danger {
  position: relative;
  flex: 1;
  min-height: 0;
  overflow: hidden;
  display: flex;
  align-items: center;
  justify-content: center;
  background:
    radial-gradient(ellipse at center, rgba(160, 10, 20, 0.55), rgba(40, 0, 4, 0.96) 70%),
    repeating-linear-gradient(0deg, rgba(255, 40, 40, 0.05) 0 1px, transparent 1px 4px),
    #120204;
  color: #ffe9ea;
  animation: pulse 2.4s ease-in-out infinite;
}
@keyframes pulse {
  50% {
    box-shadow: inset 0 0 120px rgba(255, 30, 40, 0.45);
  }
}
.tape {
  position: absolute;
  left: -10%;
  width: 120%;
  display: flex;
  justify-content: space-around;
  padding: 8px 0;
  background: #170203;
  border-block: 3px solid #ff2a36;
  box-shadow: 0 0 22px rgba(255, 30, 40, 0.6);
  color: #ff3442;
  font-weight: 900;
  font-size: 26px;
  letter-spacing: 0.04em;
  white-space: nowrap;
  text-shadow: 0 0 12px rgba(255, 40, 50, 0.85);
}
.tape b {
  margin-left: 18px;
  font-style: italic;
}
.tape.top {
  top: 6%;
  transform: rotate(-4deg);
}
.tape.bottom {
  bottom: 6%;
  transform: rotate(-4deg);
}
.middle {
  position: relative;
  z-index: 1;
  display: flex;
  flex-direction: column;
  align-items: center;
  gap: 10px;
  max-width: min(720px, 90%);
  text-align: center;
}
.sign {
  color: #ff3a46;
  filter: drop-shadow(0 0 14px rgba(255, 40, 50, 0.9));
}
h1 {
  margin: 0;
  padding: 2px 34px;
  border-inline: 2px solid rgba(255, 50, 60, 0.7);
  font-size: clamp(48px, 9vw, 92px);
  font-weight: 900;
  letter-spacing: 0.06em;
  color: #ffd6d9;
  text-shadow:
    0 0 10px #ff2a36,
    0 0 30px #ff2a36,
    0 0 60px rgba(255, 30, 40, 0.7);
}
.threat {
  margin: 4px 0 0;
  font-size: clamp(16px, 2.4vw, 24px);
  font-weight: 600;
}
.reason {
  margin: 0;
  font-size: 14px;
  color: #ffb9be;
}
.address {
  margin: 0;
  max-width: 100%;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  font-family: ui-monospace, monospace;
  font-size: 13px;
  color: #ff8f97;
}
.chips {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 10px;
  margin: 10px 0 4px;
  padding: 0;
  list-style: none;
}
.chips li {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  padding: 7px 14px;
  border-radius: 999px;
  border: 1px solid rgba(255, 255, 255, 0.14);
  background: rgba(255, 255, 255, 0.05);
  color: rgba(255, 255, 255, 0.45);
  font-size: 14px;
}
.chips li.lit {
  border-color: #ff3a46;
  background: rgba(200, 10, 20, 0.45);
  color: #fff;
  box-shadow: 0 0 16px rgba(255, 40, 50, 0.7);
}
.ways {
  display: flex;
  flex-wrap: wrap;
  justify-content: center;
  gap: 16px;
  margin-top: 14px;
}
.ways button {
  padding: 12px 34px;
  border-radius: 10px;
  font-size: 17px;
  cursor: pointer;
}
.back {
  border: 2px solid #ff4a55;
  background: linear-gradient(#d4101e, #9c0612);
  color: #fff;
  box-shadow: 0 0 22px rgba(255, 40, 50, 0.75);
}
.back:hover,
.back:focus-visible {
  box-shadow: 0 0 32px rgba(255, 60, 70, 0.95);
  outline: none;
}
.anyway {
  border: 1px solid rgba(255, 255, 255, 0.16);
  background: rgba(255, 255, 255, 0.04);
  color: rgba(255, 255, 255, 0.55);
}
.anyway:hover,
.anyway:focus-visible {
  color: rgba(255, 255, 255, 0.85);
}
.hidden {
  position: absolute;
  width: 1px;
  height: 1px;
  overflow: hidden;
  clip: rect(0 0 0 0);
}
@media (prefers-reduced-motion: reduce) {
  .danger {
    animation: none;
  }
}
</style>
