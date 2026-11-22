/**
 * The only module allowed to touch the generated contract directly.
 *
 * `bindings.ts` returns a discriminated result instead of throwing, because
 * that is what `tauri-specta` emits. Application code prefers exceptions, so
 * this layer unwraps the result into a typed `ContractError` and the stores
 * stay linear (`try { ... } catch`). Nothing else in `src/` may import
 * `@tauri-apps/api` or call `invoke` — `npm run check:arch` enforces it.
 */

import { commands } from './bindings'
import type {
  AppError,
  AppInfo,
  AppPaths,
  BackupSummary,
  ConnectionInfo,
  EngineBinary,
  EngineKind,
  EngineStatus_Serialize,
  GroupKind,
  ImportOutcome_Serialize,
  ImportPreview_Serialize,
  Language,
  Node_Deserialize,
  Node_Serialize,
  ProxyGroup_Deserialize,
  ProxyGroup_Serialize,
  ProxyMode,
  RefreshOutcome_Serialize,
  RoutingRule,
  RuleAction,
  RuleMatcher,
  Settings,
  Settings_Deserialize,
  Settings_Serialize,
  Subscription_Deserialize,
  Subscription_Serialize,
  SystemProxyStatus,
  TrafficSample,
  RuleSet_Deserialize,
  RuleSet_Serialize,
  RuleSetSource,
} from './bindings'

export class ContractError extends Error {
  readonly kind: AppError['kind']

  constructor(error: AppError) {
    super(error.message)
    this.name = 'ContractError'
    this.kind = error.kind
  }
}

type Result<T> = Promise<{ status: 'ok'; data: T } | { status: 'error'; error: AppError }>

async function unwrap<T>(pending: Result<T>): Promise<T> {
  const result = await pending
  if (result.status === 'ok') return result.data
  throw new ContractError(result.error)
}

export const api = {
  appInfo: (): Promise<AppInfo> => unwrap(commands.appInfo()),
  appPaths: (): Promise<AppPaths> => unwrap(commands.appPaths()),
  /** The OS draws the title bar; keep it in step with the CSS theme. */
  uiSetWindowTheme: async (dark: boolean): Promise<void> => {
    await unwrap(commands.uiSetWindowTheme(dark))
  },

  appOpenDataDir: async (): Promise<void> => {
    await unwrap(commands.appOpenDataDir())
  },

  appOpenLogDir: async (): Promise<void> => {
    await unwrap(commands.appOpenLogDir())
  },

  saveTextFile: (defaultName: string, contents: string): Promise<string | null> =>
    unwrap(commands.saveTextFile(defaultName, contents)),

  listNodes: (): Promise<Node_Serialize[]> => unwrap(commands.listNodes()),
  upsertNode: (node: Node_Deserialize): Promise<Node_Serialize[]> => unwrap(commands.upsertNode(node)),
  deleteNode: (id: string): Promise<Node_Serialize[]> => unwrap(commands.deleteNode(id)),
  deleteNodes: (ids: string[]): Promise<Node_Serialize[]> => unwrap(commands.deleteNodes(ids)),

  /** Reorder by one slot; the stored order is the manual order. */
  moveNode: (id: string, delta: number): Promise<Node_Serialize[]> => unwrap(commands.moveNode(id, delta)),
  exportNodeLinks: (ids: string[]): Promise<string> => unwrap(commands.exportNodeLinks(ids)),
  tagNodes: (ids: string[], tag: string, remove: boolean): Promise<Node_Serialize[]> =>
    unwrap(commands.tagNodes(ids, tag, remove)),
  testNodeLatency: (id: string): Promise<number> => unwrap(commands.testNodeLatency(id)),

  // The backend always answers with the full struct; the generated union only
  // exists because specta models both directions, so narrow it here.
  getSettings: async (): Promise<Settings_Serialize> =>
    (await unwrap(commands.getSettings())) as Settings_Serialize,
  saveSettings: async (settings: Settings_Deserialize): Promise<Settings_Serialize> =>
    (await unwrap(commands.saveSettings(settings))) as Settings_Serialize,
  autostartStatus: (): Promise<boolean> => unwrap(commands.autostartStatus()),

  importPreview: (text: string): Promise<ImportPreview_Serialize> => commands.importPreview(text),
  importNodes: (text: string): Promise<ImportOutcome_Serialize> => unwrap(commands.importNodes(text)),

  listRules: (): Promise<RoutingRule[]> => unwrap(commands.listRules()),
  upsertRule: (rule: RoutingRule): Promise<RoutingRule[]> => unwrap(commands.upsertRule(rule)),
  deleteRule: (id: string): Promise<RoutingRule[]> => unwrap(commands.deleteRule(id)),
  moveRule: (id: string, delta: number): Promise<RoutingRule[]> => unwrap(commands.moveRule(id, delta)),
  toggleRule: (id: string): Promise<RoutingRule[]> => unwrap(commands.toggleRule(id)),

  listGroups: (): Promise<ProxyGroup_Serialize[]> => unwrap(commands.listGroups()),
  upsertGroup: (group: ProxyGroup_Deserialize): Promise<ProxyGroup_Serialize[]> =>
    unwrap(commands.upsertGroup(group)),
  deleteGroup: (id: string): Promise<ProxyGroup_Serialize[]> => unwrap(commands.deleteGroup(id)),
  selectGroupMember: (id: string, nodeId: string): Promise<ProxyGroup_Serialize[]> =>
    unwrap(commands.selectGroupMember(id, nodeId)),
  toggleGroup: (id: string): Promise<ProxyGroup_Serialize[]> => unwrap(commands.toggleGroup(id)),

  engineStatus: (): Promise<EngineStatus_Serialize> => commands.engineStatus(),
  engineKinds: (): Promise<EngineKind[]> => commands.engineKinds(),
  engineBinaries: (): Promise<EngineBinary[]> => commands.engineBinaries(),

  /** Runs `version` on an installed kernel: no config, no ports, no traffic. */
  engineVersion: (kind: EngineKind): Promise<string> => unwrap(commands.engineVersion(kind)),
  engineOpenDir: async (): Promise<void> => {
    await unwrap(commands.engineOpenDir())
  },
  engineLogTail: (kind: EngineKind, lines: number): Promise<string> =>
    unwrap(commands.engineLogTail(kind, lines)),
  /** Real delay: one request through the running engine's mixed port. */
  proxyLatency: (): Promise<number> => unwrap(commands.proxyLatency()),

  /** Download throughput through the tunnel, in bytes per second. */
  proxySpeed: (): Promise<number | null> => unwrap(commands.proxySpeed()),
  /** sing-box only: drop one live connection. */
  closeConnection: (id: string): Promise<void> => unwrap(commands.closeConnection(id)).then(() => undefined),
  /** Write the whole configuration to a file the user picks; null = cancelled. */
  backupSave: (): Promise<string | null> => unwrap(commands.backupSave()),
  /** Replace the whole configuration from a picked file; null = cancelled. */
  backupLoad: (): Promise<BackupSummary | null> => unwrap(commands.backupLoad()),
  engineStart: (kind: EngineKind, nodeId: string): Promise<EngineStatus_Serialize> =>
    unwrap(commands.engineStart(kind, nodeId)),
  engineStop: (): Promise<EngineStatus_Serialize> => unwrap(commands.engineStop()),
  trafficSample: (): Promise<TrafficSample | null> => unwrap(commands.trafficSample()),
  engineConfigPreview: (kind: EngineKind, nodeId: string): Promise<string> =>
    unwrap(commands.engineConfigPreview(kind, nodeId)),

  systemProxyStatus: (): Promise<SystemProxyStatus> => unwrap(commands.systemProxyStatus()),
  systemProxyDisable: (): Promise<SystemProxyStatus> => unwrap(commands.systemProxyDisable()),

  listSubscriptions: (): Promise<Subscription_Serialize[]> => commands.listSubscriptions(),
  createSubscription: (name: string, url: string): Promise<Subscription_Serialize[]> =>
    unwrap(commands.createSubscription(name, url)),
  upsertSubscription: (subscription: Subscription_Deserialize): Promise<Subscription_Serialize[]> =>
    unwrap(commands.upsertSubscription(subscription)),
  deleteSubscription: (id: string): Promise<Subscription_Serialize[]> =>
    unwrap(commands.deleteSubscription(id)),
  refreshSubscription: (id: string): Promise<RefreshOutcome_Serialize> =>
    unwrap(commands.refreshSubscription(id)),
  refreshDue: (): Promise<RefreshOutcome_Serialize[]> => unwrap(commands.refreshDue()),

  listRuleSets: (): Promise<RuleSet_Serialize[]> => commands.listRuleSets(),
  createRuleSet: (name: string, url: string): Promise<RuleSet_Serialize[]> =>
    unwrap(commands.createRuleSet(name, url)),
  upsertRuleSet: (ruleSet: RuleSet_Deserialize): Promise<RuleSet_Serialize[]> =>
    unwrap(commands.upsertRuleSet(ruleSet)),
  deleteRuleSet: (id: string): Promise<RuleSet_Serialize[]> => unwrap(commands.deleteRuleSet(id)),
  toggleRuleSet: (id: string): Promise<RuleSet_Serialize[]> => unwrap(commands.toggleRuleSet(id)),
  setRuleSetText: (id: string, text: string): Promise<RuleSet_Serialize> =>
    unwrap(commands.setRuleSetText(id, text)),
  refreshRuleSet: (id: string): Promise<RuleSet_Serialize> => unwrap(commands.refreshRuleSet(id)),
}

/** Human-readable text for anything thrown by the backend or the network. */
export function describeError(error: unknown): string {
  if (error instanceof ContractError) return error.message
  if (error instanceof Error) return error.message
  return String(error)
}

export type {
  AppError,
  AppInfo,
  AppPaths,
  BackupSummary,
  ConnectionInfo,
  EngineBinary,
  EngineKind,
  EngineStatus_Serialize,
  GroupKind,
  ImportOutcome_Serialize,
  ImportPreview_Serialize,
  Language,
  Node_Deserialize,
  Node_Serialize,
  ProxyGroup_Deserialize,
  ProxyGroup_Serialize,
  ProxyMode,
  RefreshOutcome_Serialize,
  RoutingRule,
  RuleAction,
  RuleMatcher,
  Settings,
  Settings_Deserialize,
  Settings_Serialize,
  Subscription_Deserialize,
  Subscription_Serialize,
  SystemProxyStatus,
  TrafficSample,
  RuleSet_Deserialize,
  RuleSet_Serialize,
  RuleSetSource,
}
