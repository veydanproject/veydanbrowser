<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<!-- Context cards: Veydan entities bound to or mentioned in a note, with actions. -->
<script lang="ts">
  import Icon from '$lib/Icon.svelte';
  import { t, type TranslationKey } from '$lib/i18n';
  import { formatError } from '$lib/utils';
  import { parseBinding, isEntityKind, type EntityKind } from '$lib/bindings';
  import { ENTITY_DEFS, entitySummary, type EntityAction, type EntityStatus } from '$lib/notes-context';
  import { totpStore } from '$lib/store/totp.svelte';
  import TotpLiveCode from '$lib/components/TotpLiveCode.svelte';

  const totpPeriod = (id: string) => totpStore.list.find((e) => e.id === id)?.period ?? 30;

  const STATUS_LABEL: Record<EntityStatus, TranslationKey> = {
    ok: 'ctx_status_ok',
    bad: 'ctx_status_bad',
    unknown: 'ctx_status_unknown',
  };

  interface Props {
    /** Bindings stored on the note */
    bindings: string[];
    /** `[[kind:id]]` mentions found in the body */
    mentions: string[];
    /** Binding to highlight, e.g. after clicking a mention in the text */
    focus?: string | null;
    readonly?: boolean;
    onbind: (binding: string) => void;
    onunbind: (binding: string) => void;
  }

  let { bindings, mentions, focus = null, readonly = false, onbind, onunbind }: Props = $props();

  interface Card {
    binding: string;
    kind: EntityKind;
    id: string;
    bound: boolean;
  }

  const cards = $derived.by((): Card[] => {
    const out: Card[] = [];
    const seen = new Set<string>();
    const push = (b: string, bound: boolean) => {
      const p = parseBinding(b);
      if (!p || !isEntityKind(p.kind) || !p.value || seen.has(b)) return;
      seen.add(b);
      out.push({ binding: b, kind: p.kind, id: p.value, bound });
    };
    for (const b of bindings) push(b, true);
    for (const b of mentions) push(b, false);
    return out;
  });

  let busy = $state<string | null>(null);
  let error = $state<string | null>(null);

  function unbind(card: Card, name?: string) {
    if (!confirm($t('ctx_unbind_confirm', { name: name ?? card.id }))) return;
    onunbind(card.binding);
  }

  async function run(card: Card, action: EntityAction) {
    const key = `${card.binding}/${action.id}`;
    busy = key;
    error = null;
    try {
      await action.run(card.id);
    } catch (e) {
      error = formatError(e);
    } finally {
      busy = null;
    }
  }
</script>

{#if cards.length > 0}
  <div class="context">
    {#each cards as card (card.binding)}
      {@const def = ENTITY_DEFS[card.kind]}
      {@const entity = entitySummary(card.kind, card.id)}
      <div class="card" class:focus={focus === card.binding} class:mention={!card.bound}>
        <div class="head">
          <Icon name={def.icon} size={13} />
          <span class="name" title={entity?.name ?? card.id}>{entity?.name ?? $t('ctx_missing')}</span>
          {#if entity?.status}
            <span class="dot {entity.status}" title={$t(STATUS_LABEL[entity.status])}></span>
          {/if}
          {#if card.bound && !readonly}
            <button class="x" onclick={() => unbind(card, entity?.name)} title={$t('ctx_unbind')}>×</button>
          {/if}
        </div>
        <div class="sub">
          <span class="kind">{$t(def.label)}</span>
          {#if entity?.subtitle}<span class="sep">·</span><span class="subtitle">{entity.subtitle}</span>{/if}
        </div>
        {#if entity}
          <div class="actions">
            {#if card.kind === 'totp'}
              <TotpLiveCode entryId={card.id} period={totpPeriod(card.id)} copiedLabel={$t('totp_copy')} />
            {/if}
            {#each def.actions.filter((a) => !(card.kind === 'totp' && a.id === 'copy-code')) as action (action.id)}
              <button
                class="btn btn-ghost btn-xs"
                disabled={busy !== null}
                class:spinning={busy === `${card.binding}/${action.id}`}
                onclick={() => run(card, action)}
              >
                <Icon name={action.icon} size={11} />
                {$t(action.label)}
              </button>
            {/each}
            {#if !card.bound && !readonly}
              <button class="btn btn-ghost btn-xs bind" onclick={() => onbind(card.binding)}>
                <Icon name="plus" size={11} />
                {$t('ctx_bind')}
              </button>
            {/if}
          </div>
        {/if}
      </div>
    {/each}
    {#if error}
      <div class="error">{error}</div>
    {/if}
  </div>
{/if}

<style>
  .context {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
    gap: var(--sp-2);
    padding: var(--sp-2) var(--sp-4);
    border-top: 1px solid var(--border);
    background: var(--bg-2);
  }
  .card {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    padding: 0.45rem 0.6rem;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--bg-2);
    min-width: 0;
  }
  .card.focus { border-color: var(--accent); }
  .card.mention { border-style: dashed; }
  .head { display: flex; align-items: center; gap: 0.35rem; min-width: 0; }
  .name { font-size: var(--fs-sm); font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; flex: 1; }
  .dot { width: 7px; height: 7px; border-radius: 50%; flex-shrink: 0; }
  .dot.ok { background: var(--success); }
  .dot.bad { background: var(--danger-text); }
  .dot.unknown { background: var(--text-3); }
  .x {
    background: none; border: 0; padding: 0; cursor: pointer;
    color: var(--text-3); font-size: var(--fs-sm); line-height: 1;
  }
  .x:hover { color: var(--text); }
  .sub { display: flex; align-items: center; gap: 0.3rem; font-size: var(--fs-2xs); color: var(--text-3); min-width: 0; }
  .kind { text-transform: uppercase; letter-spacing: 0.06em; }
  .subtitle { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .actions { display: flex; flex-wrap: wrap; gap: 0.25rem; margin-top: 0.15rem; }
  .btn-xs { font-size: var(--fs-2xs); padding: 0.15rem 0.45rem; gap: 0.25rem; }
  .bind { color: var(--accent); }
  .spinning { opacity: 0.6; }
  .error { grid-column: 1 / -1; font-size: var(--fs-xs); color: var(--danger-text); }
</style>
