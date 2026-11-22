/**
 * Theme tokens for canvas-based widgets.
 *
 * ECharts draws into a canvas, so it cannot inherit CSS variables — the values
 * have to be read out of the document and handed over as strings. This module
 * is the single place that does it, so no chart ever hard-codes a colour and
 * both themes stay in sync with `tokens.css`.
 */

export interface ChartTokens {
  accent: string
  success: string
  textSecondary: string
  border: string
  textPrimary: string
}

function read(name: string, fallback: string): string {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim()
  return value || fallback
}

/** Snapshot of the colours a chart needs, for the currently applied theme. */
export function chartTokens(): ChartTokens {
  return {
    accent: read('--accent', '#4f7cff'),
    success: read('--success', '#2fa36b'),
    textSecondary: read('--text-secondary', '#6b7280'),
    textPrimary: read('--text-primary', '#111827'),
    border: read('--border-subtle', '#e5e7eb'),
  }
}
