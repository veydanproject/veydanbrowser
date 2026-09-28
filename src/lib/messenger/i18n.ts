// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

// Messenger dictionary. Merged into the app dictionary by one spread per
// locale in `$lib/i18n` so the module stays extractable. Keys: `msg_*`,
// plus `nav_messenger` for the shell entry.

export const messengerTranslations = {
  en: {
    nav_messenger: 'Messenger',
    msg_title: 'Messenger',
    msg_alpha_badge: 'alpha',
    msg_intro:
      'Decentralized end-to-end encrypted messenger on the Nostr protocol. This is stage 0: the module skeleton. Nothing connects to the network yet.',
    msg_status_title: 'Module status',
    msg_status_version: 'Messenger version',
    msg_status_schema: 'Database schema',
    msg_status_data_dir: 'Data directory',
    msg_status_secrets: 'Secret storage',
    msg_status_secrets_locked: 'locked (wired in stage 1)',
    msg_status_secrets_unlocked: 'unlocked',
    msg_status_identity: 'Identity',
    msg_status_identity_none: 'not created yet (stage 1)',
    msg_status_identity_present: 'present',
    msg_status_not_compiled: 'This build does not include the messenger module.',
    msg_status_error: 'The messenger failed to start: {error}',
    msg_settings_section: 'Messenger (alpha)',
    msg_settings_group: 'Experimental',
    msg_settings_enable: 'Show Messenger in navigation',
    msg_settings_enable_hint:
      'Experimental module. Data lives in its own database and folder; disabling hides the module without deleting anything.',
    msg_settings_not_compiled: 'Not available in this build.',
  },
  ru: {
    nav_messenger: 'Мессенджер',
    msg_title: 'Мессенджер',
    msg_alpha_badge: 'альфа',
    msg_intro:
      'Децентрализованный мессенджер со сквозным шифрованием на протоколе Nostr. Это этап 0: каркас модуля. К сети пока ничего не подключается.',
    msg_status_title: 'Состояние модуля',
    msg_status_version: 'Версия мессенджера',
    msg_status_schema: 'Схема базы данных',
    msg_status_data_dir: 'Каталог данных',
    msg_status_secrets: 'Хранилище секретов',
    msg_status_secrets_locked: 'заблокировано (подключается на этапе 1)',
    msg_status_secrets_unlocked: 'разблокировано',
    msg_status_identity: 'Идентичность',
    msg_status_identity_none: 'ещё не создана (этап 1)',
    msg_status_identity_present: 'есть',
    msg_status_not_compiled: 'Эта сборка не содержит модуль мессенджера.',
    msg_status_error: 'Мессенджер не запустился: {error}',
    msg_settings_section: 'Мессенджер (альфа)',
    msg_settings_group: 'Экспериментальное',
    msg_settings_enable: 'Показывать Мессенджер в навигации',
    msg_settings_enable_hint:
      'Экспериментальный модуль. Данные хранятся в отдельной базе и папке; выключение скрывает модуль, ничего не удаляя.',
    msg_settings_not_compiled: 'Недоступно в этой сборке.',
  },
} as const;
