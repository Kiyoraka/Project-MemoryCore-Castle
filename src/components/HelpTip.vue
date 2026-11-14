<script setup>
// A small (?) beside a setting: its explanation opens in a bubble on hover or keyboard focus,
// so the page stays short and the words are still one step away (also for screen readers).
import { useId } from 'vue'

defineProps({ text: { type: String, required: true } })
const id = `tip-${useId()}`
</script>

<template>
  <span class="help-tip">
    <button type="button" class="mark" :aria-describedby="id" aria-label="More about this" @click.prevent>?</button>
    <span :id="id" role="tooltip" class="bubble">{{ text }}</span>
  </span>
</template>

<style scoped>
.help-tip {
  position: relative;
  display: inline-flex;
  vertical-align: middle;
  margin-left: 4px;
}
.mark {
  width: 16px;
  height: 16px;
  min-height: 0;
  padding: 0;
  border: 1px solid var(--line);
  border-radius: 50%;
  background: transparent;
  color: var(--muted, var(--text));
  font-size: 10px;
  font-weight: 700;
  line-height: 14px;
  text-align: center;
  cursor: help;
}
.mark:hover,
.mark:focus-visible {
  border-color: var(--accent);
  color: var(--accent);
}
.bubble {
  position: absolute;
  top: calc(100% + 6px);
  left: -8px;
  z-index: 20;
  width: max-content;
  max-width: 280px;
  padding: 8px 10px;
  border: 1px solid var(--line);
  border-radius: 8px;
  background: var(--panel-background);
  box-shadow: 0 6px 20px rgba(0, 0, 0, 0.45);
  color: var(--text);
  font-size: 12px;
  font-weight: 400;
  letter-spacing: normal;
  line-height: 1.45;
  text-transform: none;
  white-space: normal;
  visibility: hidden;
  opacity: 0;
  transition: opacity 0.12s;
  pointer-events: none;
}
.help-tip:hover .bubble,
.mark:focus-visible + .bubble,
.mark:focus + .bubble {
  visibility: visible;
  opacity: 1;
}
</style>
