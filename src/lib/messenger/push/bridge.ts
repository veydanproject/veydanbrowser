// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

// What the screens owe to notifications: a tap opens the chat it was about,
// and an opened chat takes its notification away.

import { goto } from '$app/navigation';
import { isTauriHost, messengerApi, PUSH_TAP_EVENT } from '../api';
import { BASE, chatHref } from '../mobile/routes';
import { pushStore } from './pushStore.svelte';

let started = false;

/** What came from outside the app is checked before it is used as a route. */
const CHAT = /^group:[0-9a-f]{64}$/;

async function takeRoute(): Promise<string | null> {
  const tap = await messengerApi.push.takeTap().catch(() => null);
  if (!tap) return null;
  // A push about a direct message names no chat: the server does not know
  // who wrote. The list of chats shows which one has something new.
  return tap.chat && CHAT.test(tap.chat) ? chatHref(tap.chat) : BASE;
}

async function followTap() {
  const route = await takeRoute();
  if (route) await goto(route);
}

let launch: Promise<string | null> | null = null;
let launchClaimed = false;

/**
 * Where the tapped notification that started the app leads, if one did.
 *
 * The first to ask gets the route, everybody after gets null. The host's
 * start page asks it before sending the app to its default screen: the two
 * are one decision. Anybody else going to the chat on their own would race
 * the start page, which is shown only once the app lock lets it, and the
 * later of the two would win.
 */
export async function launchRoute(): Promise<string | null> {
  launch ??= takeRoute();
  const route = await launch;
  if (!route || launchClaimed) return null;
  launchClaimed = true;
  return route;
}

/** Started once, by the app's shell. */
export async function startPushBridge() {
  if (started) return;
  started = true;

  // Asked early, while the start page may still wait for the app lock.
  launch ??= takeRoute();

  await pushStore.load();
  if (!pushStore.view?.device.supported) return;

  // At the start address the start page follows the tap; anywhere else (a
  // page reloaded during development) nobody else will.
  if (location.pathname !== '/') {
    const route = await launchRoute();
    if (route) await goto(route);
  }
  if (isTauriHost) {
    const { listen } = await import('@tauri-apps/api/event');
    await listen(PUSH_TAP_EVENT, () => { followTap(); });
  }
}

/**
 * The user is looking at this chat, or at the list of chats: what the
 * notifications said is on the screen now.
 *
 * `group:<id>` takes away the notification of that group; anything else
 * takes away the one about direct messages, which is one for all of them.
 */
export function pushSeen(chatId?: string) {
  if (!pushStore.view?.device.supported) return;
  const key = chatId && CHAT.test(chatId) ? chatId : 'dm';
  messengerApi.push.clear(key).catch(() => {});
}
