<!-- SPDX-FileCopyrightText: 2026 Veydan Project -->
<!-- SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1 -->

<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from '$lib/i18n';
  import type { Profile, Proxy } from '$lib/types';

  interface Props {
    profiles: Profile[];
    proxies: Proxy[];
    runningProfiles: Set<string>;
  }

  let { profiles, proxies, runningProfiles }: Props = $props();

  let canvas = $state<HTMLCanvasElement>(null!);
  let width = $state(800);
  let height = $state(500);

  interface Node {
    id: string;
    label: string;
    type: 'proxy' | 'profile';
    x: number;
    y: number;
    vx: number;
    vy: number;
  }

  interface Edge { source: string; target: string; }

  let nodes = $state<Node[]>([]);
  let edges = $state<Edge[]>([]);
  let draggingNode = $state<Node | null>(null);
  let hoveredNode = $state<string | null>(null);
  let animFrame: number;

  let proxyMap = $derived(new Map(proxies.map((p) => [p.id, p])));

  let connectedProfiles = $derived(profiles.filter((p) => p.proxy_id));

  $effect(() => {
    if (connectedProfiles.length === 0) return;

    const proxyIds = [...new Set(connectedProfiles.map((p) => p.proxy_id!))]
      .filter((id) => proxyMap.has(id));

    const cx = width / 2;
    const cy = height / 2;

    nodes = [
      ...proxyIds.map((id, i) => ({
        id: `proxy-${id}`,
        label: proxyMap.get(id)?.name ?? id,
        type: 'proxy' as const,
        x: cx + Math.cos((i / proxyIds.length) * Math.PI * 2) * 150,
        y: cy + Math.sin((i / proxyIds.length) * Math.PI * 2) * 150,
        vx: 0,
        vy: 0,
      })),
      ...connectedProfiles.map((p, i) => ({
        id: `profile-${p.id}`,
        label: p.name,
        type: 'profile' as const,
        x: cx + (Math.random() - 0.5) * 300,
        y: cy + (Math.random() - 0.5) * 300,
        vx: 0,
        vy: 0,
      })),
    ];

    edges = connectedProfiles.map((p) => ({
      source: `proxy-${p.proxy_id}`,
      target: `profile-${p.id}`,
    }));

    startSim();
  });

  function startSim() {
    cancelAnimationFrame(animFrame);
    let iterations = 0;

    function tick() {
      simulate();
      draw();
      iterations++;
      if (iterations < 200 || draggingNode) {
        animFrame = requestAnimationFrame(tick);
      }
    }
    animFrame = requestAnimationFrame(tick);
  }

  function simulate() {
    const k = 80;
    const kSq = k * k;

    for (let i = 0; i < nodes.length; i++) {
      nodes[i].vx = 0;
      nodes[i].vy = 0;

      // Repulsion
      for (let j = 0; j < nodes.length; j++) {
        if (i === j) continue;
        const dx = nodes[i].x - nodes[j].x;
        const dy = nodes[i].y - nodes[j].y;
        const dist = Math.sqrt(dx * dx + dy * dy) || 1;
        const force = kSq / dist;
        nodes[i].vx += (dx / dist) * force * 0.05;
        nodes[i].vy += (dy / dist) * force * 0.05;
      }

      // Gravity to center
      nodes[i].vx += (width / 2 - nodes[i].x) * 0.01;
      nodes[i].vy += (height / 2 - nodes[i].y) * 0.01;
    }

    // Attraction along edges
    for (const edge of edges) {
      const src = nodes.find((n) => n.id === edge.source);
      const tgt = nodes.find((n) => n.id === edge.target);
      if (!src || !tgt) continue;
      const dx = tgt.x - src.x;
      const dy = tgt.y - src.y;
      const dist = Math.sqrt(dx * dx + dy * dy) || 1;
      const force = (dist - k * 1.5) * 0.03;
      const fx = (dx / dist) * force;
      const fy = (dy / dist) * force;
      if (src.id !== draggingNode?.id) { src.vx += fx; src.vy += fy; }
      if (tgt.id !== draggingNode?.id) { tgt.vx -= fx; tgt.vy -= fy; }
    }

    for (const node of nodes) {
      if (node.id === draggingNode?.id) continue;
      node.x += node.vx;
      node.y += node.vy;
      node.x = Math.max(40, Math.min(width - 40, node.x));
      node.y = Math.max(40, Math.min(height - 40, node.y));
    }
  }

  function draw() {
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const isDark = document.body.dataset.theme === 'dark';
    const edgeColor = isDark ? 'rgba(148,163,184,0.2)' : 'rgba(100,116,139,0.2)';
    const proxyFill = '#3b82f6';
    const profileFill = '#22c55e';
    const profileStoppedFill = isDark ? '#334155' : '#cbd5e1';
    const textColor = isDark ? '#e2e8f0' : '#1a202c';
    const bgColor = isDark ? '#0d1117' : '#f0f4f8';

    ctx.clearRect(0, 0, width, height);
    ctx.fillStyle = bgColor;
    ctx.fillRect(0, 0, width, height);

    // Draw edges
    ctx.strokeStyle = edgeColor;
    ctx.lineWidth = 1.5;
    for (const edge of edges) {
      const src = nodes.find((n) => n.id === edge.source);
      const tgt = nodes.find((n) => n.id === edge.target);
      if (!src || !tgt) continue;
      ctx.beginPath();
      ctx.moveTo(src.x, src.y);
      ctx.lineTo(tgt.x, tgt.y);
      ctx.stroke();
    }

    // Draw nodes
    for (const node of nodes) {
      const isHovered = node.id === hoveredNode;
      const r = node.type === 'proxy' ? (isHovered ? 18 : 14) : (isHovered ? 13 : 10);

      ctx.beginPath();
      ctx.arc(node.x, node.y, r, 0, Math.PI * 2);

      if (node.type === 'proxy') {
        ctx.fillStyle = proxyFill;
      } else {
        const profile = profiles.find((p) => `profile-${p.id}` === node.id);
        ctx.fillStyle = profile && runningProfiles.has(profile.id) ? profileFill : profileStoppedFill;
      }
      ctx.fill();

      if (isHovered) {
        ctx.strokeStyle = isDark ? 'rgba(255,255,255,0.4)' : 'rgba(0,0,0,0.3)';
        ctx.lineWidth = 2;
        ctx.stroke();
      }

      // Label
      ctx.fillStyle = textColor;
      ctx.font = `${isHovered ? '600 ' : ''}11px -apple-system, sans-serif`;
      ctx.textAlign = 'center';
      ctx.fillText(node.label.slice(0, 16), node.x, node.y + r + 13);
    }
  }

  function getNodeAt(mx: number, my: number) {
    const rect = canvas.getBoundingClientRect();
    const x = mx - rect.left;
    const y = my - rect.top;
    for (const node of [...nodes].reverse()) {
      const r = node.type === 'proxy' ? 18 : 13;
      const dx = x - node.x;
      const dy = y - node.y;
      if (dx * dx + dy * dy <= r * r) return node;
    }
    return null;
  }

  function onMouseMove(e: MouseEvent) {
    const node = getNodeAt(e.clientX, e.clientY);
    hoveredNode = node?.id ?? null;
    canvas.style.cursor = node ? 'grab' : 'default';

    if (draggingNode) {
      const rect = canvas.getBoundingClientRect();
      draggingNode.x = e.clientX - rect.left;
      draggingNode.y = e.clientY - rect.top;
      if (!animFrame) startSim();
    }
  }

  function onMouseDown(e: MouseEvent) {
    const node = getNodeAt(e.clientX, e.clientY);
    if (node) { draggingNode = node; canvas.style.cursor = 'grabbing'; startSim(); }
  }

  function onMouseUp() {
    draggingNode = null;
    canvas.style.cursor = hoveredNode ? 'grab' : 'default';
  }

  onMount(() => {
    const ro = new ResizeObserver((entries) => {
      const entry = entries[0];
      width = entry.contentRect.width;
      height = entry.contentRect.height;
      if (canvas) { canvas.width = width; canvas.height = height; }
      draw();
    });
    ro.observe(canvas.parentElement!);
    return () => { ro.disconnect(); cancelAnimationFrame(animFrame); };
  });
</script>

<div class="topology-wrap">
  {#if connectedProfiles.length === 0}
    <div class="empty">{$t('topology_empty')}</div>
  {:else}
    <div class="legend">
      <span class="legend-item proxy"><span class="dot"></span>Proxy</span>
      <span class="legend-item running"><span class="dot"></span>Running</span>
      <span class="legend-item stopped"><span class="dot"></span>Stopped</span>
    </div>
    <canvas
      bind:this={canvas}
      {width}
      {height}
      onmousemove={onMouseMove}
      onmousedown={onMouseDown}
      onmouseup={onMouseUp}
      onmouseleave={onMouseUp}
    ></canvas>
  {/if}
</div>

<style>
  .topology-wrap {
    flex: 1;
    position: relative;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }

  canvas {
    flex: 1;
    border-radius: var(--radius);
    border: 1px solid var(--border);
    display: block;
    width: 100%;
    height: 100%;
  }

  .empty {
    text-align: center;
    color: var(--text-2);
    padding: 4rem;
  }

  .legend {
    display: flex;
    gap: var(--sp-4);
    margin-bottom: var(--sp-2);
    font-size: var(--fs-xs);
    color: var(--text-2);
  }

  .legend-item { display: flex; align-items: center; gap: 0.3rem; }
  .legend-item .dot { width: 8px; height: 8px; border-radius: 50%; }
  .legend-item.proxy .dot { background: #3b82f6; }
  .legend-item.running .dot { background: #22c55e; }
  .legend-item.stopped .dot { background: var(--border-2); }
</style>
