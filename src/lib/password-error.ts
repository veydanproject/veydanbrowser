// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

import { hasErrorCode } from '$lib/utils';
import type { TranslationKey } from '$lib/i18n';

/** User-facing text for a password-vault error, or null when it is not one. */
export function passwordErrorKey(e: unknown): TranslationKey | null {
  if (hasErrorCode(e, 'vault_locked')) return 'pw_err_locked';
  if (hasErrorCode(e, 'vault_mismatch')) return 'pw_err_mismatch';
  if (hasErrorCode(e, 'decrypt_failed')) return 'pw_err_decrypt';
  if (hasErrorCode(e, 'vault_has_entries')) return 'pw_err_has_entries';
  return null;
}
