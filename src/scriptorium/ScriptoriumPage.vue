<script setup>
// The page area: the painter's PageCanvas (paint/), loaded lazily so the room never waits on it. Until it
// is there - or if it cannot load - a plain white page of the right shape stands in.
import { computed, defineAsyncComponent, ref } from 'vue'

const props = defineProps({
  // the display list as an object ({ w, h, items }), passed raw
  list: { type: Object, default: null },
  loadGlyphs: { type: Function, default: null },
  loadImage: { type: Function, default: null },
  zoom: { type: Number, default: 1 },
})
const emit = defineEmits(['update:zoom', 'pointer', 'link'])

const ready = ref(false)
const failed = ref(false)
const PageCanvas = defineAsyncComponent({
  loader: () =>
    import('./paint/PageCanvas.vue').then((m) => {
      ready.value = true
      return m
    }),
  onError(_error, _retry, fail) {
    failed.value = true
    fail()
  },
})

const painted = computed(() => !failed.value && !!props.list)
const aspect = computed(() => {
  const l = props.list
  return l && l.w > 0 && l.h > 0 ? `${l.w} / ${l.h}` : '595 / 842'
})
</script>

<template>
  <div class="page-area">
    <PageCanvas
      v-if="painted"
      class="canvas"
      :list="list"
      :load-glyphs="loadGlyphs"
      :load-image="loadImage"
      :zoom="zoom"
      @update:zoom="(z) => emit('update:zoom', z)"
      @pointer="(p) => emit('pointer', p)"
      @link="(l) => emit('link', l)"
    />
    <div v-if="!painted || !ready" class="fallback" :aria-busy="!list">
      <div class="sheet" :style="{ aspectRatio: aspect }"></div>
    </div>
  </div>
</template>

<style scoped>
.page-area {
  position: relative;
  min-width: 0;
  min-height: 0;
  height: 100%;
  overflow: hidden;
}
.canvas {
  position: absolute;
  inset: 0;
}
.fallback {
  position: absolute;
  inset: 0;
  display: grid;
  place-items: center;
  padding: 24px;
  overflow: auto;
}
.sheet {
  height: min(100%, 1100px);
  max-width: 100%;
  background: #fff;
  box-shadow: 0 8px 30px rgba(0, 0, 0, 0.35);
}
</style>
