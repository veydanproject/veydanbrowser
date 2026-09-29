<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { t } from '$lib/i18n';
  import { messengerStore } from '../store.svelte';
  import SettingsView from '../pages/SettingsView.svelte';
  import MobileFrame from './MobileFrame.svelte';
  import { BASE } from './routes';

  onMount(() => { messengerStore.ensureLoaded().catch(() => {}); });
</script>

<MobileFrame title={$t('msg_settings_title')} onback={() => goto(BASE)}>
  <SettingsView compact />
</MobileFrame>
