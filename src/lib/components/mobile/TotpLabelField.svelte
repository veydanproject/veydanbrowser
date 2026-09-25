<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import EntityLabelField from './EntityLabelField.svelte';

  interface Props {
    tags: string[];
    onchange: (tags: string[]) => void;
  }

  let { tags, onchange }: Props = $props();
</script>

<EntityLabelField {tags} {onchange} />
