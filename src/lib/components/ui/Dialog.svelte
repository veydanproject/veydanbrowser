<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import type { Snippet } from 'svelte';
  import { portal } from '$lib/portal';
  import Icon from '$lib/Icon.svelte';

  interface Props {
    open: boolean;
    title?: string;
    /** CSS width; defaults to the --dialog-w token. */
    width?: string;
    /** Close when the backdrop is clicked (default true). */
    closeOnBackdrop?: boolean;
    onclose?: () => void;
    header?: Snippet;
    children: Snippet;
    footer?: Snippet;
  }

  let {
    open = $bindable(),
    title,
    width,
    closeOnBackdrop = true,
    onclose,
    header,
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
    class="dialog-overlay"
    use:portal
    role="presentation"
    onclick={() => closeOnBackdrop && close()}
  >
    <div
      class="dialog"
      style={width ? `width:${width}` : undefined}
      role="dialog"
      aria-modal="true"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
    >
      {#if header}
        <div class="dialog-header">{@render header()}</div>
      {:else if title}
        <div class="dialog-header">
          <h2 class="dialog-title">{title}</h2>
          <button class="dialog-close" onclick={close} aria-label="Close" title="Close">
            <Icon name="x" size={16} />
          </button>
        </div>
      {/if}

      <div class="dialog-body">
        {@render children()}
      </div>

      {#if footer}
        <div class="dialog-footer">{@render footer()}</div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .dialog-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    backdrop-filter: blur(2px);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: var(--sp-4);
    z-index: var(--z-modal);
  }

  .dialog {
    display: flex;
    flex-direction: column;
    width: var(--dialog-w);
    max-width: 92vw;
    max-height: 88vh;
    background: var(--bg-2);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: var(--shadow-lg);
    overflow: hidden;
    animation: dialog-in var(--dur-base) var(--ease);
  }

  @keyframes dialog-in {
    from { opacity: 0; transform: translateY(8px) scale(0.98); }
    to   { opacity: 1; transform: none; }
  }
  @media (prefers-reduced-motion: reduce) {
    .dialog { animation: none; }
  }

  .dialog-header {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: var(--sp-4) var(--sp-5);
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .dialog-title {
    font-size: var(--fs-md);
    font-weight: var(--fw-semibold);
    color: var(--text);
  }
  .dialog-close {
    margin-left: auto;
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
  .dialog-close:hover { background: var(--surface-2); color: var(--text); }

  .dialog-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    scrollbar-gutter: stable;
    padding: var(--sp-5);
  }

  .dialog-footer {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: var(--sp-2);
    padding: var(--sp-3) var(--sp-5);
    border-top: 1px solid var(--border);
    flex-shrink: 0;
  }
</style>
