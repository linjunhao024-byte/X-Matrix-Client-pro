<script setup lang="ts">
import { computed, reactive, ref, watch } from 'vue'
import ModalDialog from './ModalDialog.vue'
import ToggleSwitch from './ToggleSwitch.vue'
import { t } from '../i18n'
import { describeError, type Node_Deserialize, type Node_Serialize } from '../services/contract'

const props = defineProps<{
  open: boolean
  node?: Node_Serialize | null
  onSubmit: (node: Node_Deserialize) => Promise<void>
}>()
const emit = defineEmits<{ close: [] }>()

type ProtocolKind = 'Vless' | 'Vmess' | 'Trojan' | 'Shadowsocks' | 'Hysteria2' | 'Tuic'
type TransportKind = 'Tcp' | 'Ws' | 'Grpc'
type SecurityKind = 'None' | 'Tls' | 'Reality'

const protocolOptions: ProtocolKind[] = ['Vless', 'Vmess', 'Trojan', 'Shadowsocks', 'Hysteria2', 'Tuic']
const transportOptions: TransportKind[] = ['Tcp', 'Ws', 'Grpc']
const ssMethods = ['aes-128-gcm', 'aes-256-gcm', 'chacha20-ietf-poly1305', '2022-blake3-aes-256-gcm']
const congestionOptions = ['', 'bbr', 'cubic', 'new_reno']
const udpRelayOptions = ['', 'native', 'quic']

/** hysteria2 and TUIC run over QUIC: the transport is quoted by the protocol
 *  itself and REALITY does not exist there, so the editor must not offer
 *  either — a stored `ws` block would be silently ignored by the kernel. */
const isQuic = (kind: ProtocolKind) => kind === 'Hysteria2' || kind === 'Tuic'

const securityOptions = computed<SecurityKind[]>(() =>
  isQuic(draft.protocolKind) ? ['Tls'] : ['None', 'Tls', 'Reality'],
)

const draft = reactive({
  id: '',
  name: '',
  server: '',
  port: 443,
  protocolKind: 'Vless' as ProtocolKind,
  uuid: '',
  flow: '',
  alterId: 0,
  password: '',
  method: 'aes-256-gcm',
  obfsPassword: '',
  upMbps: 0,
  downMbps: 0,
  congestionControl: '',
  udpRelayMode: '',
  transportKind: 'Tcp' as TransportKind,
  wsPath: '/',
  wsHost: '',
  grpcService: '',
  securityKind: 'Tls' as SecurityKind,
  sni: '',
  alpn: 'h2,http/1.1',
  allowInsecure: false,
  certPin: '',
  realityPublicKey: '',
  realityShortId: '',
  realityServerName: '',
  realityFingerprint: 'chrome',
  realitySpiderX: '/',
  tags: '',
})

/** Tags owned by a subscription: shown, but not editable here, because
 *  refresh deletes nodes by tag and a deleted tag turns the node into an
 *  orphan that no refresh will ever clean up. */
const lockedTags = ref<string[]>([])

const busy = ref(false)
const error = ref<string | null>(null)

function reset(source?: Node_Serialize | null) {
  error.value = null
  busy.value = false

  draft.id = source?.id ?? ''
  draft.name = source?.name ?? ''
  draft.server = source?.server ?? ''
  draft.port = source?.port ?? 443

  draft.protocolKind = source?.protocol.kind ?? 'Vless'
  draft.uuid = ''
  draft.flow = ''
  draft.alterId = 0
  draft.password = ''
  draft.method = 'aes-256-gcm'
  draft.obfsPassword = ''
  draft.upMbps = 0
  draft.downMbps = 0
  draft.congestionControl = ''
  draft.udpRelayMode = ''

  draft.transportKind = source?.transport.kind ?? 'Tcp'
  draft.wsPath = '/'
  draft.wsHost = ''
  draft.grpcService = ''

  draft.securityKind = source?.security.kind ?? 'Tls'
  draft.sni = ''
  draft.alpn = 'h2,http/1.1'
  draft.allowInsecure = false
  draft.certPin = ''
  draft.realityPublicKey = ''
  draft.realityShortId = ''
  draft.realityServerName = ''
  draft.realityFingerprint = 'chrome'
  draft.realitySpiderX = '/'

  // A QUIC node has no transport and always speaks TLS; normalise old data
  // here rather than letting the form show a `ws` selector that does nothing.
  if (isQuic(draft.protocolKind)) {
    draft.transportKind = 'Tcp'
    draft.securityKind = 'Tls'
  }

  const sourceTags = source?.tags ?? []
  lockedTags.value = sourceTags.filter((tag) => tag.startsWith('sub:'))
  draft.tags = sourceTags
    .filter((tag) => !tag.startsWith('sub:'))
    .join(', ')

  if (!source) return

  const protocol = source.protocol
  if (protocol.kind === 'Vless') {
    draft.uuid = protocol.uuid
    draft.flow = protocol.flow ?? ''
  } else if (protocol.kind === 'Vmess') {
    draft.uuid = protocol.uuid
    draft.alterId = protocol.alter_id
  } else if (protocol.kind === 'Trojan') {
    draft.password = protocol.password
  } else if (protocol.kind === 'Shadowsocks') {
    draft.method = protocol.method
    draft.password = protocol.password
  } else if (protocol.kind === 'Hysteria2') {
    draft.password = protocol.password
    draft.obfsPassword = protocol.obfs_password ?? ''
    draft.upMbps = protocol.up_mbps ?? 0
    draft.downMbps = protocol.down_mbps ?? 0
  } else {
    draft.uuid = protocol.uuid
    draft.password = protocol.password
    draft.congestionControl = protocol.congestion_control ?? ''
    draft.udpRelayMode = protocol.udp_relay_mode ?? ''
  }

  const transport = source.transport
  if (transport.kind === 'Ws') {
    draft.wsPath = transport.path || '/'
    draft.wsHost = transport.host ?? ''
  } else if (transport.kind === 'Grpc') {
    draft.grpcService = transport.service_name
  }

  const security = source.security
  if (security.kind === 'Tls') {
    draft.sni = security.sni ?? ''
    draft.alpn = security.alpn.join(',')
    draft.allowInsecure = security.allow_insecure
    draft.certPin = security.pinned_cert_sha256.join(', ')
  } else if (security.kind === 'Reality') {
    draft.realityPublicKey = security.public_key
    draft.realityShortId = security.short_id
    draft.realityServerName = security.server_name
    draft.realityFingerprint = security.fingerprint
    draft.realitySpiderX = security.spider_x
  }
}

watch(
  () => props.open,
  (open) => {
    if (open) reset(props.node)
  },
  { immediate: true },
)

function buildProtocol(): Node_Deserialize['protocol'] {
  switch (draft.protocolKind) {
    case 'Vless':
      return { kind: 'Vless', uuid: draft.uuid.trim(), flow: draft.flow.trim() || null }
    case 'Vmess':
      return { kind: 'Vmess', uuid: draft.uuid.trim(), alter_id: Number(draft.alterId) || 0 }
    case 'Trojan':
      return { kind: 'Trojan', password: draft.password }
    case 'Shadowsocks':
      return { kind: 'Shadowsocks', method: draft.method, password: draft.password }
    case 'Hysteria2':
      return {
        kind: 'Hysteria2',
        password: draft.password,
        obfs_password: draft.obfsPassword.trim() || null,
        // Zero reads as "no hint" to the kernel, so store absence instead.
        up_mbps: Number(draft.upMbps) > 0 ? Number(draft.upMbps) : null,
        down_mbps: Number(draft.downMbps) > 0 ? Number(draft.downMbps) : null,
      }
    case 'Tuic':
      return {
        kind: 'Tuic',
        uuid: draft.uuid.trim(),
        password: draft.password,
        congestion_control: draft.congestionControl || null,
        udp_relay_mode: draft.udpRelayMode || null,
      }
  }
}

function buildTransport(): Node_Deserialize['transport'] {
  // QUIC protocols carry no stream transport; anything but Tcp here would be
  // written into the document and then ignored by the renderer.
  if (isQuic(draft.protocolKind)) return { kind: 'Tcp' }
  switch (draft.transportKind) {
    case 'Tcp':
      return { kind: 'Tcp' }
    case 'Ws':
      return { kind: 'Ws', path: draft.wsPath.trim() || '/', host: draft.wsHost.trim() || null }
    case 'Grpc':
      return { kind: 'Grpc', service_name: draft.grpcService.trim() }
  }
}

function buildSecurity(): Node_Deserialize['security'] {
  // hysteria2 and TUIC handshake inside their own TLS; there is no plaintext
  // variant and no REALITY, so the form cannot produce anything else.
  if (isQuic(draft.protocolKind)) {
    return {
      kind: 'Tls',
      sni: draft.sni.trim() || null,
      alpn: [],
      allow_insecure: draft.allowInsecure,
      pinned_cert_sha256: draft.certPin
        .split(',')
        .map((item) => item.trim().replace(/[:\\s]/g, '').toLowerCase())
        .filter(Boolean),
    }
  }
  switch (draft.securityKind) {
    case 'None':
      return { kind: 'None' }
    case 'Tls':
      return {
        kind: 'Tls',
        sni: draft.sni.trim() || null,
        alpn: draft.alpn
          .split(',')
          .map((item) => item.trim())
          .filter(Boolean),
        // Only meaningful for TLS: self-signed certificates and bare-IP
        // servers are common, and every other client ships this switch.
        allow_insecure: draft.allowInsecure,
        pinned_cert_sha256: draft.certPin
          .split(',')
          .map((item) => item.trim().replace(/[:\s]/g, '').toLowerCase())
          .filter(Boolean),
      }
    case 'Reality':
      return {
        kind: 'Reality',
        public_key: draft.realityPublicKey.trim(),
        short_id: draft.realityShortId.trim(),
        server_name: draft.realityServerName.trim(),
        // Empty would mean "no uTLS", which reality cannot do; keep the
        // interoperable default instead of writing an empty string.
        fingerprint: draft.realityFingerprint.trim() || 'chrome',
        spider_x: draft.realitySpiderX.trim() || '/',
      }
  }
}

async function submit() {
  error.value = null

  if (!draft.name.trim()) {
    error.value = t('editor.err.name')
    return
  }
  if (!draft.server.trim()) {
    error.value = t('editor.err.server')
    return
  }
  const port = Number(draft.port)
  if (!Number.isInteger(port) || port < 1 || port > 65535) {
    error.value = t('editor.err.port')
    return
  }
  // Both QUIC protocols live or die by their credential, and a TUIC link also
  // needs the uuid; an empty one produces a kernel error the user cannot act
  // on, so name the field here.
  if (draft.protocolKind === 'Tuic' && !draft.uuid.trim()) {
    error.value = t('editor.err.uuid')
    return
  }
  if (isQuic(draft.protocolKind) && !draft.password) {
    error.value = t('editor.err.password')
    return
  }
  // A truncated fingerprint makes Xray refuse the whole config, so catch the
  // typo here instead of letting the kernel die on start.
  if (draft.securityKind === 'Tls') {
    const pins = draft.certPin
      .split(',')
      .map((item) => item.trim().replace(/[:\s]/g, ''))
      .filter(Boolean)
    if (pins.some((pin) => !/^[0-9a-fA-F]{64}$/.test(pin))) {
      error.value = t('editor.err.certPin')
      return
    }
  }

  busy.value = true
  try {
    await props.onSubmit({
      id: draft.id || crypto.randomUUID(),
      name: draft.name.trim(),
      server: draft.server.trim(),
      port,
      protocol: buildProtocol(),
      transport: buildTransport(),
      security: buildSecurity(),
      // Subscription tags ride along untouched; the user's own tags are typed
      // comma-separated, same as the certificate pins above.
      tags: [
        ...lockedTags.value,
        ...draft.tags
          .split(',')
          .map((tag) => tag.trim())
          .filter(Boolean),
      ],
    })
  } catch (cause) {
    error.value = describeError(cause)
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <ModalDialog
    :open="open"
    :title="node ? t('editor.edit') : t('editor.new')"
    :width="600"
    :close-on-overlay="false"
    @close="emit('close')"
  >
    <form class="form" @submit.prevent="submit">
      <p v-if="error" class="alert">{{ error }}</p>

      <div class="grid">
        <label class="field">
          <span class="field-label">{{ t('editor.name') }}</span>
          <input v-model="draft.name" class="input" :placeholder="t('editor.namePlaceholder')" />
        </label>
        <label class="field">
          <span class="field-label">{{ t('editor.protocol') }}</span>
          <select v-model="draft.protocolKind" class="select">
            <option v-for="kind in protocolOptions" :key="kind" :value="kind">{{ kind }}</option>
          </select>
        </label>

        <label class="field">
          <span class="field-label">{{ t('editor.server') }}</span>
          <input v-model="draft.server" class="input input-mono" :placeholder="t('editor.serverPlaceholder')" />
        </label>
        <label class="field">
          <span class="field-label">{{ t('editor.port') }}</span>
          <input v-model.number="draft.port" class="input" type="number" min="1" max="65535" />
        </label>

        <label class="field grid-span">
          <span class="field-label">{{ t('editor.tags') }}</span>
          <input v-model="draft.tags" class="input" :placeholder="t('editor.tagsPlaceholder')" />
          <span class="field-hint">{{ t('editor.tagsHint') }}</span>
          <span v-if="lockedTags.length > 0" class="field-hint">
            {{ t('editor.tagsLocked', { tags: lockedTags.join('、') }) }}
          </span>
        </label>

        <template v-if="draft.protocolKind === 'Vless'">
          <label class="field grid-span">
            <span class="field-label">{{ t('editor.uuid') }}</span>
            <input v-model="draft.uuid" class="input input-mono" placeholder="xxxxxxxx-xxxx-…" />
          </label>
          <label class="field">
            <span class="field-label">{{ t('editor.flow') }}</span>
            <select v-model="draft.flow" class="select">
              <option value="">{{ t('common.none') }}</option>
              <option value="xtls-rprx-vision">xtls-rprx-vision</option>
            </select>
          </label>
        </template>

        <template v-else-if="draft.protocolKind === 'Vmess'">
          <label class="field grid-span">
            <span class="field-label">{{ t('editor.uuid') }}</span>
            <input v-model="draft.uuid" class="input input-mono" placeholder="xxxxxxxx-xxxx-…" />
          </label>
          <label class="field">
            <span class="field-label">Alter ID</span>
            <input v-model.number="draft.alterId" class="input" type="number" min="0" />
          </label>
        </template>

        <template v-else-if="draft.protocolKind === 'Trojan'">
          <label class="field grid-span">
            <span class="field-label">{{ t('editor.password') }}</span>
            <input v-model="draft.password" class="input input-mono" type="password" />
          </label>
        </template>

        <template v-else-if="draft.protocolKind === 'Hysteria2'">
          <label class="field grid-span">
            <span class="field-label">{{ t('editor.password') }}</span>
            <input v-model="draft.password" class="input input-mono" type="password" />
          </label>
          <label class="field grid-span">
            <span class="field-label">{{ t('editor.obfsPassword') }}</span>
            <input v-model="draft.obfsPassword" class="input input-mono" type="password" />
            <span class="field-hint">{{ t('editor.obfsPasswordHint') }}</span>
          </label>
          <label class="field">
            <span class="field-label">{{ t('editor.upMbps') }}</span>
            <input v-model.number="draft.upMbps" class="input" type="number" min="0" />
          </label>
          <label class="field">
            <span class="field-label">{{ t('editor.downMbps') }}</span>
            <input v-model.number="draft.downMbps" class="input" type="number" min="0" />
          </label>
        </template>

        <template v-else-if="draft.protocolKind === 'Tuic'">
          <label class="field grid-span">
            <span class="field-label">{{ t('editor.uuid') }}</span>
            <input v-model="draft.uuid" class="input input-mono" placeholder="xxxxxxxx-xxxx-…" />
          </label>
          <label class="field grid-span">
            <span class="field-label">{{ t('editor.password') }}</span>
            <input v-model="draft.password" class="input input-mono" type="password" />
          </label>
          <label class="field">
            <span class="field-label">{{ t('editor.congestionControl') }}</span>
            <select v-model="draft.congestionControl" class="select">
              <option v-for="cc in congestionOptions" :key="cc" :value="cc">
                {{ cc || t('editor.kernelDefault') }}
              </option>
            </select>
          </label>
          <label class="field">
            <span class="field-label">{{ t('editor.udpRelayMode') }}</span>
            <select v-model="draft.udpRelayMode" class="select">
              <option v-for="mode in udpRelayOptions" :key="mode" :value="mode">
                {{ mode || t('editor.kernelDefault') }}
              </option>
            </select>
          </label>
        </template>

        <template v-else-if="draft.protocolKind === 'Shadowsocks'">
          <label class="field">
            <span class="field-label">{{ t('editor.method') }}</span>
            <select v-model="draft.method" class="select">
              <option v-for="method in ssMethods" :key="method" :value="method">{{ method }}</option>
            </select>
          </label>
          <label class="field">
            <span class="field-label">{{ t('editor.password') }}</span>
            <input v-model="draft.password" class="input input-mono" type="password" />
          </label>
        </template>

        <template v-if="isQuic(draft.protocolKind)">
          <p class="field grid-span field-hint">{{ t('editor.quicHint') }}</p>
        </template>
        <template v-else>
          <label class="field">
            <span class="field-label">{{ t('editor.transport') }}</span>
            <select v-model="draft.transportKind" class="select">
              <option v-for="kind in transportOptions" :key="kind" :value="kind">{{ kind }}</option>
            </select>
          </label>
        </template>
        <label v-if="draft.transportKind === 'Ws'" class="field">
          <span class="field-label">{{ t('editor.wsPath') }}</span>
          <input v-model="draft.wsPath" class="input input-mono" placeholder="/" />
        </label>
        <label v-if="draft.transportKind === 'Ws'" class="field">
          <span class="field-label">{{ t('editor.wsHost') }}</span>
          <input v-model="draft.wsHost" class="input input-mono" :placeholder="t('editor.optional')" />
        </label>
        <label v-if="draft.transportKind === 'Grpc'" class="field grid-span">
          <span class="field-label">{{ t('editor.grpcService') }}</span>
          <input v-model="draft.grpcService" class="input input-mono" placeholder="service-name" />
        </label>

        <label class="field">
          <span class="field-label">{{ t('editor.security') }}</span>
          <select v-model="draft.securityKind" class="select">
            <option v-for="kind in securityOptions" :key="kind" :value="kind">{{ kind }}</option>
          </select>
        </label>
        <template v-if="draft.securityKind === 'Tls'">
          <label class="field">
            <span class="field-label">{{ t('editor.sni') }}</span>
            <input v-model="draft.sni" class="input input-mono" :placeholder="t('editor.optional')" />
          </label>
          <label class="field">
            <span class="field-label">{{ t('editor.alpn') }}</span>
            <input v-model="draft.alpn" class="input input-mono" placeholder="h2,http/1.1" />
          </label>
          <label class="field grid-span">
            <ToggleSwitch v-model="draft.allowInsecure" :label="t('editor.allowInsecure')" :description="t('editor.allowInsecureHint')" />
          </label>
          <label class="field grid-span">
            <span class="field-label">{{ t('editor.certPin') }}</span>
            <input v-model="draft.certPin" class="input input-mono" :placeholder="t('editor.certPinPlaceholder')" />
            <span class="field-hint">{{ t('editor.certPinHint') }}</span>
          </label>
        </template>
        <template v-else-if="draft.securityKind === 'Reality'">
          <label class="field">
            <span class="field-label">{{ t('editor.serverName') }}</span>
            <input v-model="draft.realityServerName" class="input input-mono" />
          </label>
          <label class="field grid-span">
            <span class="field-label">{{ t('editor.publicKey') }}</span>
            <input v-model="draft.realityPublicKey" class="input input-mono" />
          </label>
          <label class="field">
            <span class="field-label">{{ t('editor.shortId') }}</span>
            <input v-model="draft.realityShortId" class="input input-mono" />
          </label>
          <label class="field">
            <span class="field-label">{{ t('editor.fingerprint') }}</span>
            <input
              v-model="draft.realityFingerprint"
              class="input input-mono"
              placeholder="chrome"
            />
          </label>
          <label class="field">
            <span class="field-label">{{ t('editor.spiderX') }}</span>
            <input v-model="draft.realitySpiderX" class="input input-mono" placeholder="/" />
          </label>
        </template>
      </div>
    </form>

    <template #footer>
      <button class="btn" type="button" @click="emit('close')">{{ t('common.cancel') }}</button>
      <button class="btn btn-primary" type="button" :disabled="busy" @click="submit">
        {{ busy ? t('common.saving') : t('common.save') }}
      </button>
    </template>
  </ModalDialog>
</template>

<style scoped>
.form {
  display: flex;
  flex-direction: column;
  gap: 14px;
}

.grid {
  display: grid;
  grid-template-columns: 1fr 1fr;
  gap: 14px;
}

.grid-span {
  grid-column: 1 / -1;
}
</style>
