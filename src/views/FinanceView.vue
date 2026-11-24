<script setup>
// FINANCE: the receipts your companion reads, kept on this computer only. Built out in the
// next step (Waiting, This month, Latest, Export and the drop zone).
defineProps({ companion: { type: String, default: 'Your companion' } })
</script>

<template>
  <section class="finance" aria-label="Finance">
    <h1>FINANCE</h1>
    <p class="quiet">Receipts {{ companion }} reads, kept on this computer only.</p>
  </section>
</template>

<style scoped>
.finance {
  padding: 24px;
}

.quiet {
  color: var(--text-dim);
}
</style>
