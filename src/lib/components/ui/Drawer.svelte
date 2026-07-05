<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import type { Snippet } from 'svelte';
  import { portal } from '$lib/portal';
  import Icon from '$lib/Icon.svelte';

  interface Props {
    open: boolean;
    title?: string;
    /** CSS width; defaults to the --drawer-w token. Use --drawer-w-lg for wide. */
    width?: string;
    side?: 'right' | 'left';
    closeOnBackdrop?: boolean;
    onclose?: () => void;
    /** Extra controls rendered in the header, before the close button. */
    actions?: Snippet;
    /** Fixed region between the header and the scrolling body (e.g. a tab bar or toolbar). */
    subheader?: Snippet;
    children: Snippet;
    footer?: Snippet;
  }

  let {
    open = $bindable(),
    title,
    width,
    side = 'right',
    closeOnBackdrop = true,
    onclose,
    actions,
    subheader,
    children,
    footer,
  }: Props = $props();

  function close() {
    open = false;
    onclose?.();
  }

  function onKeydown(e: KeyboardEvent) {
    if (e.key === 'Escape' && open) {
      e.stopPropagation();
      close();
    }
  }
</script>

<svelte:window onkeydown={onKeydown} />

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_noninteractive_element_interactions -->
  <div
    class="drawer-backdrop"
    use:portal
    role="presentation"
    onclick={() => closeOnBackdrop && close()}
  >
    <div
      class="drawer {side}"
      style={width ? `width:${width}` : undefined}
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
    >
      {#if title || actions}
        <header class="drawer-header">
          {#if title}<h2 class="drawer-title">{title}</h2>{/if}
          <div class="drawer-actions">
            {@render actions?.()}
            <button class="drawer-close" onclick={close} aria-label="Close" title="Close">
              <Icon name="x" size={16} />
            </button>
          </div>
        </header>
      {/if}

      {#if subheader}
        <div class="drawer-subheader">{@render subheader()}</div>
      {/if}

      <div class="drawer-body">
        {@render children()}
      </div>

      {#if footer}
        <footer class="drawer-footer">{@render footer()}</footer>
      {/if}
    </div>
  </div>
{/if}

<style>
  .drawer-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.4);
    z-index: var(--z-drawer);
    display: flex;
  }
  .drawer-backdrop:has(.left) { justify-content: flex-start; }
  .drawer-backdrop:has(.right) { justify-content: flex-end; }

  .drawer {
    display: flex;
    flex-direction: column;
    width: var(--drawer-w);
    max-width: 92vw;
    height: 100%;
    background: var(--bg-2);
    box-shadow: var(--shadow-lg);
  }
  .drawer.right { border-left: 1px solid var(--border); animation: drawer-in-right var(--dur-base) var(--ease); }
  .drawer.left  { border-right: 1px solid var(--border); animation: drawer-in-left  var(--dur-base) var(--ease); }

  @keyframes drawer-in-right { from { transform: translateX(100%); } to { transform: none; } }
  @keyframes drawer-in-left  { from { transform: translateX(-100%); } to { transform: none; } }
  @media (prefers-reduced-motion: reduce) {
    .drawer { animation: none; }
  }

  .drawer-header {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: var(--sp-3) var(--sp-4);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .drawer-title {
    font-size: var(--fs-md);
    font-weight: var(--fw-semibold);
    color: var(--text);
  }
  .drawer-actions {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin-left: auto;
  }
  .drawer-close {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-2);
    border: none;
    flex-shrink: 0;
  }
  .drawer-close:hover { background: var(--surface-2); color: var(--text); }

  .drawer-subheader {
    flex-shrink: 0;
  }

  .drawer-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    scrollbar-gutter: stable;
    padding: var(--sp-4);
  }

  .drawer-footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--sp-2);
    padding: var(--sp-3) var(--sp-4);
    border-top: 1px solid var(--border);
    flex-shrink: 0;
  }
</style>
