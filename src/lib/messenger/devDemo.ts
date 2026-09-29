// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

// Demo data for the browser preview (`pnpm dev` without Tauri). Enabled by
// `localStorage['messenger.demo'] = '1'`. Never used inside the app.

import type { ChatMode, MessengerChat, MessengerContact, MessengerIdentity, MessengerMessage, MessengerProfile } from './api';

const ME = 'ab'.repeat(32);
const hex = (c: string) => c.repeat(64).slice(0, 64);
const npub = (pk: string, tag: string) => `npub1${tag}${pk}`.slice(0, 63);

function profile(pk: string, name: string, about: string, nip05: string | null): MessengerProfile {
  return {
    pubkey: pk, npub: npub(pk, name.toLowerCase().replace(/[^a-z]/g, '')), name: name.toLowerCase(), display_name: name, about,
    picture: null, banner: null, website: null, nip05, lud16: null, nip05_verified: !!nip05, event_created_at: 1, fetched_at: 1,
  };
}

export interface DemoData {
  identity: MessengerIdentity;
  ownProfile: MessengerProfile;
  contacts: MessengerContact[];
  chats: MessengerChat[];
  messages: Record<string, MessengerMessage[]>;
}

export function demoEnabled(): boolean {
  try { return typeof localStorage !== 'undefined' && localStorage.getItem('messenger.demo') === '1'; }
  catch { return false; }
}

export function buildDemo(): DemoData {
  const now = Math.floor(Date.now() / 1000);
  const people: [string, string, string, string | null, ChatMode, boolean][] = [
    [hex('1a'), 'Алиса Морозова', 'Дизайнер. Пишу редко, но по делу.', 'alice@veydan.net', 'full_chat', true],
    [hex('2b'), 'Борис', 'Бэкенд, реле, инфраструктура.', null, 'full_chat', true],
    [hex('3c'), 'Вера К.', '', null, 'request_received', false],
    [hex('4d'), 'Глеб', '', null, 'request_sent', false],
    [hex('5e'), 'Спам-бот', '', null, 'blocked', false],
    [hex('6f'), 'Дарья', 'На связи по будням.', 'daria@example.org', 'removed_by_peer', false],
  ];
  const contacts: MessengerContact[] = [];
  const chats: MessengerChat[] = [];
  const messages: Record<string, MessengerMessage[]> = {};
  let n = 0;
  const msg = (chat: string, from: string, at: number, text: string | null, extra: Partial<MessengerMessage> = {}): MessengerMessage => ({
    id: `demo${(n++).toString(16).padStart(60, '0')}`, chat_id: chat, direction: from === ME ? 'out' : 'in',
    status: from === ME ? 'sent' : 'received', content_type: 'text', text, sender_pubkey: from, reply_to: null,
    created_at: at, edited_at: null, deleted: false, failure_reason: null, media: null, ...extra,
  });

  for (const [pk, name, about, nip05, mode, canSend] of people) {
    const p = profile(pk, name, about, nip05);
    const isContact = mode === 'full_chat' || mode === 'request_sent' || mode === 'removed_by_peer';
    if (isContact) {
      contacts.push({ pubkey: pk, npub: p.npub, nickname: null, note: null, is_muted: name === 'Борис', notification_level: 'all', followed: name === 'Алиса Морозова', profile: p, created_at: now - 86400 * 9, updated_at: now - 3600 });
    }
    const id = `dm:${pk}`;
    chats.push({ id, kind: 'dm', peer_pubkey: pk, peer_npub: p.npub, title: name, picture: null, is_contact: isContact, is_muted: name === 'Борис', unread: 0, last_message_at: null, last_preview: null, pinned: name === 'Алиса Морозова', archived: false, mode, can_send: canSend });
    messages[id] = [];
  }

  const day = 86400;
  const [alice, boris, vera, gleb, bot, daria] = people.map((p) => p[0]);
  const a = `dm:${alice}`;
  const m1 = msg(a, alice, now - day * 2 - 4000, 'Привет! Посмотрела макеты мессенджера. В целом очень нравится, но есть пара мыслей по списку чатов.');
  const m2 = msg(a, ME, now - day * 2 - 3900, 'Привет! Давай, рассказывай.');
  messages[a].push(
    msg(a, '', now - day * 2 - 4100, 'request_accepted', { content_type: 'system', direction: 'out' }),
    m1, m2,
    msg(a, alice, now - day * 2 - 3800, 'Первое: время последнего сообщения лучше прижать вправо. Второе: счётчик непрочитанных у заглушённых чатов сделать серым, а не акцентным.'),
    msg(a, alice, now - day * 2 - 3790, 'И ещё: закреплённые чаты сверху.'),
    msg(a, ME, now - day * 2 - 3600, 'Согласен по всем трём пунктам. Сделаю сегодня.', { reply_to: { id: m1.id, sender_pubkey: alice, text: m1.text } }),
    msg(a, ME, now - day - 7200, 'Готово, обновил. Посмотри, когда будет минута.', { edited_at: now - day - 7100 }),
    msg(a, alice, now - day - 7000, null, { deleted: true }),
    msg(a, alice, now - day - 6900, 'Смотрю.'),
    msg(a, alice, now - 5400, 'Макет экрана', { content_type: 'media', media: { name: 'chat-list-v3.png', mime: 'image/png', size: 842_113, kind: 'image' } }),
    msg(a, ME, now - 5000, null, { content_type: 'media', media: { name: 'Техническое задание (черновик).pdf', mime: 'application/pdf', size: 2_412_004, kind: 'file', local_path: '/home/dev/spec.pdf' } }),
    msg(a, ME, now - 1800, 'Отлично выглядит. Беру в работу 👍'),
    msg(a, ME, now - 600, 'Это сообщение не ушло: реле было недоступно.', { status: 'failed', failure_reason: 'no relay accepted' }),
    msg(a, ME, now - 60, 'А это ждёт отправки.', { status: 'queued' }),
  );
  const b = `dm:${boris}`;
  messages[b].push(
    msg(b, boris, now - 9000, 'NIP-77 на node-1 включу на этой неделе.'),
    msg(b, ME, now - 8900, 'Хорошо. Код менять не придётся: догон истории сам переключится на Negentropy.'),
    msg(b, boris, now - 300, 'Логи реле за ночь', { content_type: 'media', media: { name: 'relay-2026-09-29.log', mime: 'text/plain', size: 104_857_600, kind: 'file' } }),
    msg(b, boris, now - 200, 'Посмотри, там есть странные всплески около трёх ночи.'),
  );
  const v = `dm:${vera}`;
  messages[v].push(
    msg(v, '', now - 4001, 'request_received', { content_type: 'system', direction: 'out' }),
    msg(v, vera, now - 4000, 'Здравствуйте! Мне дали ваш ключ на конференции, хочу обсудить интеграцию.'),
  );
  const g = `dm:${gleb}`;
  messages[g].push(
    msg(g, ME, now - day * 3, 'Глеб, привет! Это я, напиши, когда появишься.'),
    msg(g, '', now - day * 3 + 1, 'request_sent', { content_type: 'system', direction: 'out' }),
  );
  messages[`dm:${bot}`].push(
    msg(`dm:${bot}`, bot, now - day * 5, 'Вы выиграли приз! Перейдите по ссылке…'),
    msg(`dm:${bot}`, '', now - day * 5 + 60, 'blocked', { content_type: 'system', direction: 'out' }),
  );
  const d = `dm:${daria}`;
  messages[d].push(
    msg(d, daria, now - day * 12, 'Спасибо за помощь с настройкой!'),
    msg(d, ME, now - day * 12 + 100, 'Обращайся.'),
    msg(d, '', now - day * 6, 'contact_left', { content_type: 'system', direction: 'out' }),
  );

  const unread: Record<string, number> = { [b]: 2, [v]: 1 };
  for (const c of chats) {
    const list = messages[c.id].filter((x) => x.content_type !== 'system');
    const last = list[list.length - 1];
    if (last) {
      c.last_message_at = last.created_at;
      c.last_preview = last.deleted ? null : last.content_type === 'media' ? `📎 ${last.text ?? (last.media?.name as string)}` : last.text;
    }
    c.unread = unread[c.id] ?? 0;
  }

  const ownProfile = profile(ME, 'Виталий', 'Строю Veydan Space.', null);
  return {
    identity: { npub: ownProfile.npub, pubkey: ME, created_at: now - day * 30 },
    ownProfile, contacts, chats, messages,
  };
}
