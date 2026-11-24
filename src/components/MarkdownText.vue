<script setup>
import { computed } from 'vue'
import { parseBlocks, parseSpans } from './markdown.js'
import { linkWay, openLink } from './links.js'

// The companion's markdown drawn as plain elements: paragraphs, headings, lists, quotes, rules, code and
// tables. Text only, never raw HTML. A link opens on click when it is one Castle may open (links.js). `size` is 'chat' (inside a bubble) or 'reader' (a page).
const props = defineProps({
  text: { type: String, default: '' },
  size: { type: String, default: 'chat' },
})

const blocks = computed(() => parseBlocks(props.text))
</script>

<template>
  <!-- md-chat / md-page: names no parent uses, since a parent's scoped styles reach this root. -->
  <div class="md" :class="size === 'reader' ? 'md-page' : 'md-chat'">
    <template v-for="(block, n) in blocks" :key="n">
      <pre v-if="block.kind === 'code'" class="code">{{ block.text }}</pre>
      <hr v-else-if="block.kind === 'rule'" />
      <div v-else-if="block.kind === 'table'" class="table-wrap">
        <table>
          <thead>
            <tr>
              <th v-for="(cell, c) in block.head" :key="c">
                <template v-for="(span, s) in parseSpans(cell)" :key="s"><strong v-if="span.bold">{{ span.text }}</strong><code v-else-if="span.code">{{ span.text }}</code><em v-else-if="span.em">{{ span.text }}</em><a v-else-if="linkWay(span.link)" class="link" :href="span.link" :title="span.link" @click.prevent="openLink(span.link).catch(() => {})">{{ span.text }}</a><span v-else :title="span.link">{{ span.text }}</span></template>
              </th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="(row, r) in block.rows" :key="r">
              <td v-for="(cell, c) in row" :key="c">
                <template v-for="(span, s) in parseSpans(cell)" :key="s"><strong v-if="span.bold">{{ span.text }}</strong><code v-else-if="span.code">{{ span.text }}</code><em v-else-if="span.em">{{ span.text }}</em><a v-else-if="linkWay(span.link)" class="link" :href="span.link" :title="span.link" @click.prevent="openLink(span.link).catch(() => {})">{{ span.text }}</a><span v-else :title="span.link">{{ span.text }}</span></template>
              </td>
            </tr>
          </tbody>
        </table>
      </div>
      <component
        :is="block.kind === 'item' ? 'div' : block.kind === 'quote' ? 'blockquote' : block.kind === 'p' ? 'p' : 'div'"
        v-else
        :class="block.kind"
        :style="block.kind === 'item' ? { paddingLeft: 16 + block.depth * 16 + 'px' } : null"
      >
        <span v-if="block.kind === 'item'" class="marker" aria-hidden="true">{{ block.box ? (block.checked ? '☑' : '☐') : block.number || '•' }}</span>
        <template v-for="(span, s) in parseSpans(block.text)" :key="s"><strong v-if="span.bold">{{ span.text }}</strong><code v-else-if="span.code">{{ span.text }}</code><em v-else-if="span.em">{{ span.text }}</em><a v-else-if="linkWay(span.link)" class="link" :href="span.link" :title="span.link" @click.prevent="openLink(span.link).catch(() => {})">{{ span.text }}</a><span v-else :class="{ link: span.link }" :title="span.link">{{ span.text }}</span></template>
      </component>
    </template>
  </div>
</template>

<style scoped>
.md > * + * {
  margin-top: 6px;
}

.p {
  margin: 0;
  white-space: pre-wrap;
}

.h1,
.h2,
.h3,
.h4 {
  font-weight: 600;
  line-height: 1.3;
}

/* A document, whatever the page around it centres. */
.md {
  text-align: left;
}

.md-chat .h1 { font-size: 16px; }
.md-chat .h2 { font-size: 15px; }
.md-chat .h3,
.md-chat .h4 { font-size: 14px; }

.md-page .h1 { margin-bottom: 6px; font-size: 22px; }
.md-page .h2 { margin-top: 18px; font-size: 17px; color: color-mix(in srgb, var(--accent, #b48cff) 45%, var(--text, #f3efff)); }
.md-page .h3,
.md-page .h4 { margin-top: 12px; font-size: 15px; color: color-mix(in srgb, var(--accent, #b48cff) 65%, var(--text, #f3efff)); }

.item {
  position: relative;
}

.md > .item + .item {
  margin-top: 2px;
}

.marker {
  position: absolute;
  margin-left: -16px;
  color: var(--accent, #b48cff);
}

blockquote {
  margin: 0;
  padding-left: 10px;
  border-left: 3px solid var(--accent, #b48cff);
  color: var(--text-dim, #d4cbf0);
}

hr {
  margin: 10px 0;
  border: 0;
  border-top: 1px solid var(--line, rgba(200, 180, 255, 0.25));
}

code,
.code {
  font-family: 'Cascadia Mono', Consolas, ui-monospace, Menlo, monospace;
  font-size: 12.5px;
  background: rgba(0, 0, 0, 0.28);
  border-radius: 5px;
}

code {
  padding: 1px 5px;
}

.code {
  margin: 0;
  padding: 8px 10px;
  overflow-x: auto;
  line-height: 1.35;
  white-space: pre;
}

.table-wrap {
  max-width: 100%;
  overflow-x: auto;
}

table {
  border-collapse: collapse;
  font-size: 13px;
}

th,
td {
  padding: 4px 10px;
  border: 1px solid var(--line, rgba(200, 180, 255, 0.25));
  text-align: left;
  vertical-align: top;
}

th {
  font-weight: 600;
  background: rgba(255, 255, 255, 0.06);
}

.link {
  text-decoration: underline;
  text-decoration-color: var(--accent, #b48cff);
}
a.link {
  color: var(--accent, #8fb8ff);
  cursor: pointer;
}
a.link:hover {
  text-decoration-thickness: 2px;
}
</style>
