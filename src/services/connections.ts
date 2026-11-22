/**
 * Aggregations over the live connection table.
 *
 * The kernel reports one row per connection, which answers "what is happening
 * right now" but not "where is my traffic going". Clash groups the same table
 * by rule and by process; neither works for us — sing-box exposes no process
 * field, and its route rules cannot carry a tag (`route.rules[].tag` is an
 * unknown field, verified against sing-box 1.14.4), so every connection is
 * labelled with the same "final". The two groupings that are always populated
 * are the destination host and the outbound chain.
 */
import type { ConnectionInfo } from './bindings'

export type GroupKey = 'host' | 'chain'

export interface ConnectionGroup {
  /** Display name: the rule tag, the chain, or a placeholder for "none". */
  name: string
  /** True when the kernel reported no value for this key. */
  unknown: boolean
  count: number
  upload: number
  download: number
}

/**
 * Sum the live table by rule or by outbound chain.
 *
 * Missing byte counts are treated as zero: the kernel omits them for
 * connections it has not accounted yet, and `undefined + n` would make the
 * whole group NaN.
 */
export function groupConnections(rows: ConnectionInfo[], key: GroupKey): ConnectionGroup[] {
  const buckets = new Map<string, ConnectionGroup>()

  for (const row of rows) {
    // The first chain entry is the outbound actually used; the rest is the
    // group path that led to it. Grouping by the full chain would split one
    // outbound into several rows for no benefit.
    const raw = key === 'host' ? row.host : (row.chains[0] ?? '')
    const name = raw && raw.length > 0 ? raw : ''
    const bucket = buckets.get(name) ?? {
      name,
      unknown: name === '',
      count: 0,
      upload: 0,
      download: 0,
    }
    bucket.count += 1
    bucket.upload += row.upload ?? 0
    bucket.download += row.download ?? 0
    buckets.set(name, bucket)
  }

  // Busiest first: the point of the view is to find the rule eating the link.
  return [...buckets.values()].sort(
    (a, b) => b.download + b.upload - (a.download + a.upload) || b.count - a.count,
  )
}
