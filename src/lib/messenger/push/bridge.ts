// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

// What the screens owe to notifications: a tap opens the chat it was about,
// an opened chat takes its notification away, and the push server writes its
// texts in the language of the app.

import { goto } from '$app/navigation';
import { get } from 'svelte/store';
import { locale } from '$lib/i18n';
import { isTauriHost, messengerApi, PUSH_TAP_EVENT } from '../api';
import { BASE, chatHref } from '../mobile/routes';
import { pushStore } from './pushStore.svelte';

let started = false;

/** What came from outside the app is checked before it is used as a route. */
const CHAT = /^group:[0-9a-f]{64}$/;

async function followTap() {
  const tap = await messengerApi.push.takeTap().catch(() => null);
  if (!tap) return;
  // A push about a direct message names no chat: the server does not know
  // who wrote. The list of chats shows which one has something new.
  await goto(tap.chat && CHAT.test(tap.chat) ? chatHref(tap.chat) : BASE);
}

/** Started once, by the first screen of the messenger that is shown. */
export async function startPushBridge() {
  if (started) return;
  started = true;

  locale.subscribe((l) => { pushStore.setLocale(l); });
  await pushStore.load();
  if (!pushStore.view?.device.supported) return;

  // The tap that opened the app happened before there was anybody to tell.
  await followTap();
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

/** The language a test would expect, without waiting for the store. */
export const currentLocale = () => get(locale);
