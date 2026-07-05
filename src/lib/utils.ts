// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

import type { AppError } from '$lib/types';

export function formatError(e: unknown): string {
  if (e != null && typeof e === 'object' && 'message' in e) {
    return (e as AppError).message;
  }
  return String(e);
}
