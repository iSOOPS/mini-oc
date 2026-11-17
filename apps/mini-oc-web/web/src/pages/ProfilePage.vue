<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { api } from '../api'
import { useAuthStore } from '../store'

const auth = useAuthStore()

const key = ref('')
const showKey = ref(false)
const copied = ref(false)
const loadingKey = ref(false)
const keyError = ref('')

onMounted(async () => {
  await auth.probe().catch(() => {})
  await loadKey()
})

async function loadKey() {
  loadingKey.value = true
  keyError.value = ''
  try {
    const r = await api.meKey()
    key.value = r.key
  } catch (e: any) {
    keyError.value = e?.message ?? '获取密钥失败'
  } finally {
    loadingKey.value = false
  }
}

async function copyKey() {
  if (!key.value) return
  try {
    await navigator.clipboard.writeText(key.value)
    copied.value = true
    setTimeout(() => (copied.value = false), 1500)
  } catch {
    /* 剪贴板不可用则静默忽略 */
  }
}
</script>

<template>
  <div class="page">
    <header class="head">
      <h1>我的</h1>
    </header>

    <div class="card profile-head">
      <div class="avatar">{{ auth.user?.name?.charAt(0)?.toUpperCase() ?? '?' }}</div>
      <div class="who">
        <strong class="name">{{ auth.user?.name ?? '—' }}</strong>
        <span class="uid">用户 ID：{{ auth.user?.id ?? '—' }}</span>
      </div>
    </div>

    <div class="card">
      <h3 class="sec-title">基本信息</h3>
      <dl class="info">
        <div class="row">
          <dt>用户名</dt>
          <dd>{{ auth.user?.name ?? '—' }}</dd>
        </div>
        <div class="row">
          <dt>用户 ID</dt>
          <dd class="mono">{{ auth.user?.id ?? '—' }}</dd>
        </div>
        <div class="row">
          <dt>设备数</dt>
          <dd>{{ auth.devices.length }}</dd>
        </div>
      </dl>
    </div>

    <div class="card">
      <h3 class="sec-title">用户密钥</h3>
      <p class="desc">用于在客户端 mini-oc 中绑定设备、建立穿透连接的凭据，请妥善保管。</p>
      <div class="key-box">
        <code class="key">{{ loadingKey ? '加载中…' : (showKey ? key : '••••••••••••••••••••••••••••••••') }}</code>
        <div class="key-actions">
          <button class="btn ghost small" :disabled="loadingKey || !key" @click="showKey = !showKey">
            {{ showKey ? '隐藏' : '显示' }}
          </button>
          <button class="btn small" :disabled="!key" @click="copyKey">{{ copied ? '已复制' : '复制' }}</button>
        </div>
      </div>
      <p v-if="keyError" class="err">{{ keyError }}</p>
    </div>
  </div>
</template>

<style scoped>
.page { max-width: 720px; margin: 0 auto; display: flex; flex-direction: column; gap: 14px; }
.head h1 { font-size: 26px; letter-spacing: -0.5px; }

.profile-head { display: flex; align-items: center; gap: 16px; }
.avatar {
  width: 56px; height: 56px; border-radius: 50%;
  background: var(--primary); color: #fff;
  display: flex; align-items: center; justify-content: center;
  font-size: 26px; font-weight: 700; flex-shrink: 0;
}
.who { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
.who .name { font-size: 20px; }
.who .uid { font-size: 13px; color: var(--text-2); }

.sec-title { font-size: 15px; font-weight: 700; margin-bottom: 12px; }
.info { display: flex; flex-direction: column; }
.info .row { display: flex; justify-content: space-between; padding: 10px 0; border-bottom: 1px solid var(--surface-2); }
.info .row:last-child { border-bottom: none; }
.info dt { font-size: 14px; color: var(--text-2); }
.info dd { font-size: 14px; font-weight: 500; word-break: break-all; text-align: right; }
.mono { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; }

.desc { font-size: 13px; color: var(--text-2); margin-bottom: 12px; }
.key-box { display: flex; flex-direction: column; gap: 10px; }
.key {
  background: var(--surface-2); padding: 12px; border-radius: 10px;
  font-size: 15px; word-break: break-all; font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
  letter-spacing: 0.5px;
}
.key-actions { display: flex; gap: 8px; }
.err { color: var(--red); font-size: 13px; margin-top: 8px; }
</style>
