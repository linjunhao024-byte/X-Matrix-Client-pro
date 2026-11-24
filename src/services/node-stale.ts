/**
 * Does the saved node still render the same way as the one the kernel runs?
 *
 * A node is rendered into the kernel config once, when the kernel starts, so
 * editing the node that is currently running (its server, port, transport or
 * security) and pressing 保存 leaves the kernel proxying with the old values
 * until it is restarted. Nothing in the store says so — this helper is the
 * decision the nodes page makes after a successful save.
 *
 * The module imports types only (erased at compile time) so it stays testable
 * in plain node, like the other `src/services` helpers.
 */
import type { Node_Deserialize, Node_Serialize } from './contract'

/**
 * The subset of a node that reaches the rendered kernel config.
 *
 * `name` is display only and `tags` only group the card on this page, so
 * neither appears here: renaming or re-tagging the running node must NOT
 * restart it. Both sides of the contract are accepted — the editor submits a
 * `_Deserialize` and the store hands back a `_Serialize`.
 */
export type RenderableNode = {
  server: string
  port: number
  protocol: Node_Serialize['protocol'] | Node_Deserialize['protocol']
  transport: Node_Serialize['transport'] | Node_Deserialize['transport']
  security: Node_Serialize['security'] | Node_Deserialize['security']
}

/**
 * True when the two versions of the same node would render differently, i.e.
 * when a running kernel is still serving the `before` settings.
 *
 * Scalars compare by value. The three tagged unions carry the per-protocol
 * credentials plus the transport and security options, and both versions come
 * from the same editor, so key order is stable and their serialised form is a
 * faithful comparison — picking fields by hand would silently ignore every
 * option the contract gains later.
 */
export function rendersDifferently(before: RenderableNode, after: RenderableNode): boolean {
  if (before.server !== after.server) return true
  if (before.port !== after.port) return true
  return (
    JSON.stringify(before.protocol) !== JSON.stringify(after.protocol) ||
    JSON.stringify(before.transport) !== JSON.stringify(after.transport) ||
    JSON.stringify(before.security) !== JSON.stringify(after.security)
  )
}
