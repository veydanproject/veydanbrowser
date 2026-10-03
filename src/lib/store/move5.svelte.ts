// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

// The move to Veydan Space 5. 4.0.x is the last line of this repository:
// version 5 is released in Veydan-Space and does not open this version's
// local data, so the data moves through sync (the vault format is the same).

/** Where version 5 is downloaded from. */
export const SPACE5_URL = 'https://github.com/veydanproject/Veydan-Space/releases/latest';

/**
 * The banner comes back on every launch until the user moves: hidden only
 * for this run of the app. The card in Settings is always there.
 */
export const move5 = $state({ bannerHidden: false });
