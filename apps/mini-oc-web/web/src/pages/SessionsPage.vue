<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { useRoute } from 'vue-router'
import { api, type DualSource, type Session } from '../api'
import { useAuthStore } from '../store'

const route = useRoute()
const auth = useAuthStore()

const deviceName = ref<string>('')
const directory = ref<string>('')
const result = ref<DualSource<Session> | null>(null)
const busy = ref(false)
const error = ref('')
const search = ref('')
const jumpInfo = ref<{ url: string; expiresIn: number } | null>(null)
const jumpBlocked = ref(false)

onMounted(async () => {
  deviceName.value = (route.query.device as string) || ''
  directory.value = (route.query.directory as string) || ''
  if (!deviceName.value) {
    await auth.probe().catch(() => {})
    deviceName.value = auth.onlineDevices[0]?.name ?? auth.devices[0]?.name ?? ''
  }
  await load()
})

watch(() => route.query, () => {
  const d = route.query.device as string
  const dir = route.query.directory as string
  if (d) deviceName.value = d
  if (dir !== undefined) directory.value = dir
  load()
})

async function load() {
  if (!deviceName.value) return
  busy.value = true
  error.value = ''
  try {
    result.value = await api.deviceSessions(deviceName.value, directory.value || undefined)
  } catch (e: any) {
    error.value = e?.message ?? '加载失败'
  } finally {
    busy.value = false
  }
}

const filtered = computed(() => {
  const list = result.value?.data ?? []
  const q = search.value.trim().toLowerCase()
  if (!q) return list
  return list.filter(
    (s) => (s.title ?? '').toLowerCase().includes(q) || s.directory.toLowerCase().includes(q),
  )
})

function fmtTime(ts: number) {
  const d = new Date(ts)
  const diff = Date.now() - ts
  if (diff < 3600_000) return `${Math.max(1, Math.round(diff / 60_000))} 分钟前`
  if (diff < 86400_000) return `${Math.round(diff / 3600_000)} 小时前`
  return d.toLocaleDateString()
}

async function jump(s?: Session) {
  if (!deviceName.value) return
  const dir = s?.directory || directory.value
  if (!dir) return
  try {
    const info = await api.jump(deviceName.value, dir, s?.id)
    jumpInfo.value = info
    jumpBlocked.value = false
    // window.open 被拦截时降级为手动链接展示（V1 行为）
    const win = window.open(info.url, '_blank', 'noopener')
    if (!win) jumpBlocked.value = true
  } catch (e: any) {
    error.value = e?.message ?? '跳转失败（设备可能离线）'
  }
}
</script>

<template>
  <div class="page">
    <header class="head">
      <h1>会话</h1>
      <button class="btn small" @click="jump()">➜ 在设备上打开</button>
    </header>

    <div v-if="result?.source === 'snapshot'" class="banner">
      快照数据 · 获取于 {{ result.fetchedAt ? new Date(result.fetchedAt).toLocaleString() : '未知时间' }} · 设备当前离线
    </div>

    <div v-if="directory" class="dir">📂 {{ directory }}</div>
    <input v-model="search" placeholder="🔍 检索会话标题或路径" />

    <div v-if="jumpInfo" class="card jump">
      <template v-if="jumpBlocked">
        <p>弹窗被拦截，请手动打开（一次性链接，{{ jumpInfo.expiresIn / 60 }} 分钟内有效）：</p>
        <a :href="jumpInfo.url" target="_blank" rel="noopener" class="link">{{ jumpInfo.url }}</a>
      </template>
      <template v-else>
        <p>已在新窗口打开 ✓（链接一次性，{{ jumpInfo.expiresIn / 60 }} 分钟内有效）</p>
      </template>
    </div>

    <div v-if="error" class="card err">{{ error }}</div>
    <div v-else-if="busy" class="card empty">加载中…</div>
    <div v-else-if="filtered.length === 0" class="card empty">暂无会话</div>

    <transition-group name="fade" tag="div" class="list">
      <button v-for="s in filtered" :key="s.id" class="card sess" @click="jump(s)">
        <div class="title">{{ s.title || '（无标题会话）' }}</div>
        <div class="sub">
          <span class="dir-short">{{ s.directory.split('/').slice(-1)[0] }}</span>
          <span>·</span>
          <span>{{ fmtTime(s.time.updated) }}</span>
        </div>
      </button>
    </transition-group>
  </div>
</template>

<style scoped>
.page { max-width: 720px; margin: 0 auto; display: flex; flex-direction: column; gap: 12px; }
.head { display: flex; align-items: center; justify-content: space-between; }
.head h1 { font-size: 26px; letter-spacing: -0.5px; }
.banner { background: var(--primary-weak); color: var(--primary); border-radius: var(--radius-m); padding: 10px 14px; font-size: 13px; }
.dir { font-size: 13px; color: var(--text-2); word-break: break-all; }
.list { display: flex; flex-direction: column; gap: 10px; }
.sess { text-align: left; cursor: pointer; border: none; font: inherit; color: inherit; transition: transform 0.12s ease; }
.sess:active { transform: scale(0.99); }
.title { font-weight: 600; }
.sub { display: flex; gap: 8px; font-size: 13px; color: var(--text-2); margin-top: 4px; }
.jump { font-size: 14px; }
.jump p { margin-bottom: 8px; }
.link { color: var(--primary); word-break: break-all; }
.empty { text-align: center; color: var(--text-2); padding: 28px; }
.err { color: var(--red); }
</style>
