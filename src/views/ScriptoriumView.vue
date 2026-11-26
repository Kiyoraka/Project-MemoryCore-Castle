<script setup>
// SCRIPTORIUM in the Castle: the host-agnostic room shell (src/scriptorium) given the Castle's calls to
// its Rust edge (scriptorium_host.rs), the layout from Settings, and the room's own chat session. The room
// opening writes its rules and starts the door's inbox; the room closing ends its session, stops the inbox
// and closes its documents.
import { onBeforeUnmount, onMounted, ref } from 'vue'
import TerminalPanel from '../components/TerminalPanel.vue'
import ScriptoriumRoom from '../scriptorium/ScriptoriumRoom.vue'
import {
  onScriptoriumChanged,
  scriptoriumClose,
  scriptoriumCloseDoc,
  scriptoriumCommand,
  scriptoriumDisplay,
  scriptoriumDoor,
  scriptoriumGlyphs,
  scriptoriumHistory,
  scriptoriumImage,
  scriptoriumNew,
  scriptoriumOpen,
  scriptoriumOpenDialog,
  scriptoriumPrepare,
  scriptoriumRecent,
  scriptoriumRedo,
  scriptoriumSave,
  scriptoriumSaveDialog,
  scriptoriumUndo,
} from '../api/castle.js'

defineProps({
  layout: { type: String, default: 'classic' },
  companion: { type: String, default: 'Your companion' },
  cli: { type: String, default: 'claude' },
  visible: { type: Boolean, default: true },
})

const ready = ref(false)
const stamp = ref(0)
const problem = ref('')

// the room shell's api: the Castle's edge to the scriptorium crate
const api = {
  newDoc: scriptoriumNew,
  closeDoc: scriptoriumCloseDoc,
  describe: (doc) => scriptoriumDoor(doc, 'describe'),
  display: scriptoriumDisplay,
  glyphs: scriptoriumGlyphs,
  image: scriptoriumImage,
  command: scriptoriumCommand,
  undo: scriptoriumUndo,
  redo: scriptoriumRedo,
  history: scriptoriumHistory,
  pick: scriptoriumOpenDialog,
  open: scriptoriumOpen,
  pickSave: scriptoriumSaveDialog,
  save: scriptoriumSave,
  recent: scriptoriumRecent,
  onChanged: onScriptoriumChanged,
}

async function prepare() {
  try {
    await scriptoriumPrepare()
    problem.value = ''
    ready.value = true
    stamp.value += 1
  } catch (reason) {
    problem.value = String(reason)
  }
}

onMounted(prepare)

onBeforeUnmount(() => {
  // the room closed: its session ends, the door's inbox stops, its documents close
  scriptoriumClose().catch(() => {})
})
</script>

<template>
  <div class="scriptorium-view">
    <ScriptoriumRoom :layout="layout" :api="api" :ai-name="companion">
      <template #chat>
        <div class="chat" style="--panel-left: 0px; --panel-top: 0px; --panel-bottom: 0px; --panel-width: 100%">
          <TerminalPanel v-if="ready" room="scriptorium" :settings-stamp="stamp" :companion="companion" :cli="cli" @setup="prepare" @settings="prepare" />
          <p v-else-if="problem" class="problem">{{ problem }}</p>
        </div>
      </template>
    </ScriptoriumRoom>
  </div>
</template>

<style scoped>
.scriptorium-view {
  height: 100%;
  min-height: 0;
  box-sizing: border-box;
}
.chat {
  position: absolute;
  inset: 0;
  overflow: hidden;
}
.problem {
  margin: 16px;
  opacity: 0.8;
}
</style>
