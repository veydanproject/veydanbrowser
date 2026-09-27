<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  export type SettingsNavGroup = {
    label: string;
    items: { id: string; label: string }[];
  };

  let {
    groups,
    active,
    onselect,
  }: {
    groups: SettingsNavGroup[];
    active: string;
    onselect: (id: string) => void;
  } = $props();
</script>

<!-- Sticky section index for the settings page -->
<nav class="settings-nav">
  {#each groups as group (group.label)}
    <div class="nav-group">
      <div class="section-label">{group.label}</div>
      {#each group.items as item (item.id)}
        <button
          type="button"
          class="nav-item"
          class:active={active === item.id}
          onclick={() => onselect(item.id)}
        >
          {item.label}
        </button>
      {/each}
    </div>
  {/each}
</nav>

<style>
  .settings-nav {
    position: sticky;
    top: 0;
    align-self: start;
    display: flex;
    flex-direction: column;
    gap: var(--sp-4);
    width: 200px;
    padding-top: var(--sp-2);
    max-height: calc(100vh - var(--topbar-h) - var(--sp-6) * 2);
    overflow-y: auto;
    scrollbar-width: thin;
  }
  .nav-group { display: flex; flex-direction: column; gap: 2px; }
  .section-label { padding: 0 var(--sp-3); margin-bottom: var(--sp-1); }
  .nav-item {
    display: block; width: 100%; text-align: left;
    padding: 0.4rem var(--sp-3);
    background: none; border: none; border-radius: var(--radius-sm);
    color: var(--text-2); font-size: var(--fs-sm); cursor: pointer;
    transition: background 0.15s, color 0.15s;
  }
  .nav-item:hover { background: var(--surface-2); color: var(--text); }
  .nav-item.active { background: var(--accent-bg); color: var(--accent-text); font-weight: var(--fw-semibold); }
</style>
