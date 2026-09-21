// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

/** Pixels the IME covers at the bottom of the layout viewport. */
export function keyboardBottom(): number {
  const vv = window.visualViewport;
  if (!vv) return 0;
  return Math.max(0, Math.round(window.innerHeight - vv.height - vv.offsetTop));
}

/** Subscribe to IME height changes. Calls `fn` immediately. */
export function onKeyboard(fn: (bottom: number) => void): () => void {
  const vv = window.visualViewport;
  const tick = () => fn(keyboardBottom());
  vv?.addEventListener('resize', tick);
  vv?.addEventListener('scroll', tick);
  window.addEventListener('resize', tick);
  tick();
  return () => {
    vv?.removeEventListener('resize', tick);
    vv?.removeEventListener('scroll', tick);
    window.removeEventListener('resize', tick);
  };
}
