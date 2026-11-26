<script setup>
// The one Sweep sheet of the window, opened from HOME's Sweep card or the DEV room's Free disk
// space… (which opens it at the Developer section). Its state is useSweep's.
import { computed } from 'vue'
import { useSweep } from '../../composables/useSweep.js'
import { resolveLook } from './sweep.js'
import SweepSheet from './SweepSheet.vue'

const props = defineProps({
  themeLook: { type: String, default: '' },
})

const sweep = useSweep()
const { groups, status, ticks, cleaning, freedSoFar, result, deepBusy, problem, tickedTotal, sheetOpen, sheetDeveloper } = sweep
const look = computed(() => resolveLook(status.value.settings.look, props.themeLook))
</script>

<template>
  <SweepSheet
    v-if="sheetOpen"
    :look="look"
    :groups="groups"
    :ticks="ticks"
    :ticked-total="tickedTotal"
    :cleaning="cleaning"
    :freed-so-far="freedSoFar"
    :result="result"
    :problem="problem"
    :deep-offered="status.deep"
    :deep-busy="deepBusy"
    :developer="sheetDeveloper"
    @toggle="sweep.toggle"
    @clean="sweep.clean"
    @deep="sweep.deep"
    @close="sweep.closeSheet"
  />
</template>
