/**
 * CSV for the live connection table.
 *
 * Written by hand rather than with a library: the table has seven columns and
 * the only subtle part is quoting. A spreadsheet is also the one place where
 * a stray comma silently shifts every column, so the escaping has to be
 * exactly right — hence the tests next door.
 */
import type { ConnectionInfo } from './bindings'

const HEADERS = ['host', 'network', 'rule', 'chains', 'upload', 'download', 'start'] as const

/**
 * Quote a field when a spreadsheet would otherwise misread it.
 *
 * RFC 4180: wrap in double quotes and double any quote inside. Commas,
 * quotes and newlines all force quoting; a bare value stays bare so the file
 * is still readable in a terminal.
 */
export function csvField(value: string): string {
  if (/[",\n\r]/.test(value)) {
    return `"${value.replace(/"/g, '""')}"`
  }
  return value
}

/**
 * Render connections as CSV. Missing numbers become empty cells rather than
 * `undefined`, which is what a spreadsheet would show as text.
 */
export function connectionsToCsv(rows: ConnectionInfo[]): string {
  const lines = [HEADERS.join(',')]
  for (const row of rows) {
    lines.push(
      [
        csvField(row.host ?? ''),
        csvField(row.network ?? ''),
        csvField(row.rule ?? ''),
        csvField(row.chains.join(' > ')),
        row.upload === null || row.upload === undefined ? '' : String(row.upload),
        row.download === null || row.download === undefined ? '' : String(row.download),
        csvField(row.start ?? ''),
      ].join(','),
    )
  }
  // Trailing newline: every POSIX tool and spreadsheet copes, and it stops the
  // last row from running into the next append.
  return `${lines.join('\n')}\n`
}
