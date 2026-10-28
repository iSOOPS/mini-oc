<script setup lang="ts">
import { onMounted } from 'vue'
import { useAuthStore } from '../store'
import type { DeviceStatus } from '../api'

const auth = useAuthStore()

onMounted(() => {
  // 静默首载（ADR 17：进入页面无 spinner 打扰）
  auth.probe().catch(() => {})
})

async function refresh() {
  await auth.probe()
}

function dotClass(status?: DeviceStatus) {
  if (status === 'online') return 'green'
  if (status === 'auth_mismatch') return 'orange'
  return 'red'
}

function statusText(status?: DeviceStatus) {
  if (status === 'online') return '在线'
  if (status === 'auth_mismatch') return '凭据不一致'
  return '离线'
}
</script>

<template>
  <div class="page">
    <header class="head">
      <h1>设备</h1>
      <button class="btn ghost small" :disabled="auth.loading" @click="refresh">
        {{ auth.loading ? '刷新中…' : '↻ 刷新状态' }}
      </button>
    </header>

    <div class="card" v-for="d in auth.devices" :key="d.name">
      <div class="row1">
        <span class="dot" :class="dotClass(d.status)" />
        <div class="title">
          <strong>{{ d.desc }}</strong>
          <span class="name">{{ d.deviceName || d.name }}</span>
        </div>
        <span class="badge" :data-s="d.status">{{ statusText(d.status) }}</span>
      </div>
      <dl class="meta">
        <div><dt>平台</dt><dd>{{ d.pctype }}</dd></div>
        <div><dt>端口</dt><dd>{{ d.port }}</dd></div>
        <div><dt>版本</dt><dd>{{ d.version ?? '—' }}</dd></div>
        <div><dt>绑定</dt><dd>{{ d.bound ? '已绑定' : '未绑定' }}</dd></div>
      </dl>
      <p v-if="d.status === 'auth_mismatch'" class="hint warn">
        凭据不一致：用户密钥已重置，请在设备端重启云服务。
      </p>
      <p v-if="d.status === 'offline'" class="hint">设备离线：请确认设备端 mini-oc 已启动服务。</p>
    </div>

    <div v-if="!auth.loading && auth.devices.length === 0" class="card empty">
      暂无设备——请在客户端设置页上报设备
    </div>
  </div>
</template>

<style scoped>
.page { max-width: 720px; margin: 0 auto; display: flex; flex-direction: column; gap: 14px; }
.head { display: flex; align-items: center; justify-content: space-between; }
.head h1 { font-size: 26px; letter-spacing: -0.5px; }
.row1 { display: flex; align-items: center; gap: 12px; }
.title { flex: 1; display: flex; flex-direction: column; }
.title .name { font-size: 13px; color: var(--text-2); }
.badge { font-size: 13px; font-weight: 600; }
.badge[data-s='online'] { color: var(--green); }
.badge[data-s='auth_mismatch'] { color: var(--orange); }
.badge[data-s='offline'] { color: var(--red); }
.meta { display: grid; grid-template-columns: repeat(4, 1fr); gap: 8px; margin-top: 14px; }
.meta dt { font-size: 12px; color: var(--text-2); }
.meta dd { font-size: 14px; margin-top: 2px; }
.hint { margin-top: 10px; font-size: 13px; color: var(--text-2); }
.hint.warn { color: var(--orange); }
.empty { text-align: center; color: var(--text-2); padding: 32px; }
</style>
