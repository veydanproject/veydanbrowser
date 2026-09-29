// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

// Push notifications as the screens see them: one state, read by the
// settings panel and by the card that offers pushes.

import {
  messengerApi,
  messengerError,
  pushErrorCode,
  type MessengerPushTest,
  type MessengerPushView,
} from '../api';

/** Key of the text for a failure: `msg_push_error_<code>`, or the general one. */
export function pushErrorKey(error: string, known: (key: string) => boolean): string {
  const code = pushErrorCode(error);
  const key = code ? `msg_push_error_${code}` : '';
  return key && known(key) ? key : 'msg_push_error_other';
}

class PushStore {
  view = $state<MessengerPushView | null>(null);
  busy = $state(false);
  /** A failure of the last action, as the runtime worded it. */
  error = $state('');
  test = $state<MessengerPushTest | null>(null);

  /** Pushes can be turned on here: a phone, with a push service. */
  get possible(): boolean {
    return Boolean(this.view?.device.supported && this.view.device.available);
  }

  /** The user was never asked, and there is something to offer. */
  get toOffer(): boolean {
    const v = this.view;
    return Boolean(v && this.possible && !v.status.offered && !v.status.enabled);
  }

  private async run(action: () => Promise<MessengerPushView | void>) {
    this.error = '';
    this.busy = true;
    try {
      const view = await action();
      if (view) this.view = view;
    } catch (e) {
      this.error = messengerError(e);
    } finally {
      this.busy = false;
    }
  }

  load = () => this.run(() => messengerApi.push.status());
  refresh = () => this.run(() => messengerApi.push.refresh());
  setEnabled = (on: boolean) => {
    this.test = null;
    return this.run(() => messengerApi.push.setEnabled(on));
  };
  setPrefs = (dm: boolean, groups: boolean) => this.run(() => messengerApi.push.setPrefs(dm, groups));
  setServer = (url: string | null) => this.run(() => messengerApi.push.setServer(url));

  /** "Not now": the question is not asked again. */
  decline = () =>
    this.run(async () => {
      await messengerApi.push.markOffered();
      return messengerApi.push.status();
    });

  sendTest = () =>
    this.run(async () => {
      this.test = null;
      this.test = await messengerApi.push.test();
      return messengerApi.push.status();
    });

  /** The texts of pushes are written by the server, in this language. */
  async setLocale(locale: string) {
    try {
      await messengerApi.push.setLocale(locale);
    } catch {
      // Not worth a word to the user: pushes come in English until it works.
    }
  }
}

export const pushStore = new PushStore();
