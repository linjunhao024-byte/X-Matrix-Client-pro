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
  ExitIp,
  GroupKind,
  ImportOutcome_Serialize,
  ImportPreview_Serialize,
  Language,
  LatencyProbe_Serialize,
  GeoAssetStatus_Serialize,
  GeoUpdate,
  LogDelta,
  Node_Deserialize,
  Node_Serialize,
  NodeProbe_Serialize,
  ProbeKind,
  ProxyGroup_Deserialize,
  ProxyGroup_Serialize,
  ProcessInfo,
  ProxyMode,
  RefreshOutcome_Serialize,
  RuleTest_Serialize,
  RoutingRule,
  RuleAction,
  RuleMatcher,
  Settings,
  Settings_Deserialize,
  Settings_Serialize,
  Subscription_Deserialize,
  Subscription_Serialize,
  SystemProxyStatus,
  TrafficPoint,
  TrafficSample,
  RuleSet_Deserialize,
  RuleSet_Serialize,
  RuleSetSource,
  UpdateStatus,
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

  /** Write the redacted support report; `null` means they cancelled. */
  exportDiagnostics: (): Promise<string | null> => unwrap(commands.exportDiagnostics()),

  /** Ask the release feed; never touches the installed binary. */
  checkUpdate: (): Promise<UpdateStatus> => unwrap(commands.checkUpdate()),

  /**
   * Which address the outside world sees, and whether the request went through
   * the running tunnel. `via` is decided in Rust, next to the kernel state.
   */
  exitIp: (): Promise<ExitIp> => unwrap(commands.exitIp()),

  /** Hand a page (the release notes) to the system browser. */
  appOpenUrl: (url: string): Promise<void> => unwrap(commands.appOpenUrl(url)).then(() => undefined),

  /** Read a text file the user picked; `null` means they cancelled. */
  openTextFile: (): Promise<string | null> => unwrap(commands.openTextFile()),

  /** Running processes merged by executable name, for the rule editor. */
  listProcesses: (): Promise<ProcessInfo[]> => unwrap(commands.listProcesses()),

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
  /** An empty list means every node: the backend reads its own list then. */
  testNodesLatency: (ids: string[]): Promise<LatencyProbe_Serialize[]> =>
    unwrap(commands.testNodesLatency(ids)),
  /**
   * The geo data files a `geoip:`/`geosite:` rule needs, and whether they are
   * on disk. A missing file is reported, not thrown.
   */
  geoAssets: (): Promise<GeoAssetStatus_Serialize[]> => commands.geoAssets(),
  /**
   * Download both files. `lines` reports one line per file (failures included),
   * `restartNeeded` says the running kernel still holds the old copies — the
   * data files are read once, when a kernel starts.
   */
  updateGeoAssets: (): Promise<GeoUpdate> => unwrap(commands.updateGeoAssets()),

  /** Every remembered latency result, keyed by node id; survives a restart.
   *  A read that cannot fail: the backend hands back its own map, so this is
   *  the one probe call that is not wrapped in `Result`. */
  nodeProbes: (): Promise<Record<string, NodeProbe_Serialize>> => commands.nodeProbes(),
  /** Forget remembered results for these ids; an empty list clears them all. */
  clearNodeProbes: (ids: string[]): Promise<Record<string, NodeProbe_Serialize>> =>
    unwrap(commands.clearNodeProbes(ids)),

  // The backend always answers with the full struct; the generated union only
  // exists because specta models both directions, so narrow it here.
  getSettings: async (): Promise<Settings_Serialize> =>
    (await unwrap(commands.getSettings())) as Settings_Serialize,
  saveSettings: async (settings: Settings_Deserialize): Promise<Settings_Serialize> =>
    (await unwrap(commands.saveSettings(settings))) as Settings_Serialize,
  /**
   * Change only the language. The picker applies immediately, so it must not
   * send the whole form: unsaved edits on the page would be saved with it.
   */
  setLanguage: async (language: Language): Promise<Language> =>
    (await unwrap(commands.setLanguage(language))) as Language,
  autostartStatus: (): Promise<boolean> => unwrap(commands.autostartStatus()),

  importPreview: (text: string): Promise<ImportPreview_Serialize> => commands.importPreview(text),
  importNodes: (text: string): Promise<ImportOutcome_Serialize> => unwrap(commands.importNodes(text)),

  listRules: (): Promise<RoutingRule[]> => unwrap(commands.listRules()),
  upsertRule: (rule: RoutingRule): Promise<RoutingRule[]> => unwrap(commands.upsertRule(rule)),

  /** Which rule would catch this host, and what it would do. */
  testDomain: (domain: string): Promise<RuleTest_Serialize> => unwrap(commands.testDomain(domain)),
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

  /** Native picker for one kernel; `null` when the user cancels. */
  pickEngineBinary: (kind: EngineKind): Promise<string | null> =>
    unwrap(commands.pickEngineBinary(kind)),

  /** Runs `version` on an installed kernel: no config, no ports, no traffic. */
  engineVersion: (kind: EngineKind): Promise<string> => unwrap(commands.engineVersion(kind)),
  engineOpenDir: async (): Promise<void> => {
    await unwrap(commands.engineOpenDir())
  },
  engineLogTail: (kind: EngineKind, lines: number): Promise<string> =>
    unwrap(commands.engineLogTail(kind, lines)),

  /**
   * What the kernel appended since `since` bytes.
   *
   * `size` comes back as `number | null` in the generated contract (specta
   * renders every f64 that way), so both fields are coalesced here rather than
   * at each call site.
   */
  engineLogDelta: async (kind: EngineKind, since: number): Promise<LogDelta> => {
    const delta = await unwrap(commands.engineLogDelta(kind, since))
    return { text: delta.text, size: delta.size ?? 0, reset: delta.reset }
  },
  engineLogClear: async (kind: EngineKind): Promise<void> => {
    await unwrap(commands.engineLogClear(kind))
  },
  /** Real delay: one request through the running engine's mixed port. */
  proxyLatency: (): Promise<number> => unwrap(commands.proxyLatency()),

  /** Download throughput through the tunnel, in bytes per second. */
  proxySpeed: (): Promise<number | null> => unwrap(commands.proxySpeed()),
  /** Real delay of one strategy group, via the running sing-box Clash API. */
  groupDelay: (id: string): Promise<number> => unwrap(commands.groupDelay(id)),
  /** Real delay of one node, via its own outbound in the running sing-box. */
  nodeDelay: (id: string): Promise<number> => unwrap(commands.nodeDelay(id)),
  /** The same number with no kernel running: one throwaway kernel, one request. */
  nodeDelayOffline: (id: string): Promise<number> => unwrap(commands.nodeDelayOffline(id)),
  /** sing-box only: drop one live connection. */
  closeConnection: (id: string): Promise<void> => unwrap(commands.closeConnection(id)).then(() => undefined),
  /** sing-box only: drop several connections; an empty list means all of them. */
  closeConnections: (ids: string[]): Promise<void> =>
    unwrap(commands.closeConnections(ids)).then(() => undefined),
  /** Write the whole configuration to a file the user picks; null = cancelled. */
  backupSave: (): Promise<string | null> => unwrap(commands.backupSave()),
  /** Replace the whole configuration from a picked file; null = cancelled. */
  backupLoad: (): Promise<BackupSummary | null> => unwrap(commands.backupLoad()),
  engineStart: (kind: EngineKind, nodeId: string): Promise<EngineStatus_Serialize> =>
    unwrap(commands.engineStart(kind, nodeId)),
  engineApply: (kind: EngineKind, nodeId: string): Promise<EngineStatus_Serialize> =>
    unwrap(commands.engineApply(kind, nodeId)),
  engineStop: (): Promise<EngineStatus_Serialize> => unwrap(commands.engineStop()),
  trafficSample: (): Promise<TrafficSample | null> => unwrap(commands.trafficSample()),
  // No Result envelope: this command cannot fail, so the binding returns the
  // list directly (unlike the ones whose Rust side is `AppResult<…>`).
  trafficHistory: (minutes: number): Promise<TrafficPoint[]> =>
    commands.trafficHistory(minutes),
  trafficClearHistory: (): Promise<void> =>
    unwrap(commands.trafficClearHistory()).then(() => undefined),
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
  UpdateStatus,
  RuleTest_Serialize,
  AppError,
  AppInfo,
  AppPaths,
  BackupSummary,
  ConnectionInfo,
  EngineBinary,
  EngineKind,
  EngineStatus_Serialize,
  ExitIp,
  GroupKind,
  ImportOutcome_Serialize,
  ImportPreview_Serialize,
  Language,
  GeoAssetStatus_Serialize,
  LatencyProbe_Serialize,
  Node_Deserialize,
  Node_Serialize,
  NodeProbe_Serialize,
  ProbeKind,
  ProxyGroup_Deserialize,
  ProxyGroup_Serialize,
  ProcessInfo,
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
