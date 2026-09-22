<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  interface Props {
    kind: string;
  }

  let { kind }: Props = $props();

  const LETTER: Record<string, string> = {
    folder: 'f',
    workspace: 'w',
    profile: 'p',
    domain: 's',
    tag: 't',
  };

  const letter = $derived(LETTER[kind] ?? '');
</script>

{#if letter}
  <span class="mark" aria-hidden="true">{letter}</span>
{/if}

<style>
  /* Theme color, not the chip color. */
  .mark {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 13px;
    height: 13px;
    border-radius: 50%;
    border: 1px solid var(--text);
    color: var(--text);
    background: transparent;
    font-size: 8px;
    font-weight: 700;
    line-height: 1;
    flex-shrink: 0;
  }
</style>
