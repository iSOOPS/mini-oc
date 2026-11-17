<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { api, type DualSource, type Project } from '../api'
import { useAuthStore } from '../store'
import { useRoute, useRouter } from 'vue-router'

const auth = useAuthStore()
const router = useRouter()
const route = useRoute()

// 单设备模式：设备名来自路由参数（从设备卡片「项目」按钮进入）
const deviceName = ref<string>('')
const deviceDesc = ref<string>('')
const result = ref<DualSource<Project> | null>(null)
const busy = ref(false)
const error = ref('')
const search = ref('')

onMounted(async () => {
  deviceName.value = (route.params.name as string) || ''
  if (!deviceName.value) {
    // 兜底：无设备名时回退设备列表
    await auth.probe().catch(() => {})
    deviceName.value = auth.onlineDevices[0]?.name ?? auth.devices[0]?.name ?? ''
  }
  const dev = auth.devices.find((d) => d.name === deviceName.value)
  deviceDesc.value = dev ? (dev.desc || dev.deviceName) : deviceName.value
  await load()
})

async function load() {
  if (!deviceName.value) return
  busy.value = true
  error.value = ''
  try {
    result.value = await api.deviceProjects(deviceName.value)
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
    (p) => p.canonical.toLowerCase().includes(q) || (p.name ?? '').toLowerCase().includes(q),
  )
})

function openSessions(p: Project) {
  router.push({
    path: `/devices/${deviceName.value}/sessions`,
    query: { directory: p.canonical },
  })
}

function back() {
  router.push('/devices')
}

function fmtTime(canonical: string) {
  return canonical.split('/').slice(-2).join('/')
}
</script>

<template>
  <div class="page">
    <header class="head">
      <div class="title-block">
        <button class="btn ghost small" @click="back">‹ 返回</button>
        <h1>项目</h1>
        <span class="dev-tag">{{ deviceDesc }}（{{ deviceName }}）</span>
      </div>
    </header>

    <div v-if="result?.source === 'snapshot'" class="banner">
      快照数据 · 获取于 {{ result.fetchedAt ? new Date(result.fetchedAt).toLocaleString() : '未知时间' }}
    </div>

    <input v-model="search" placeholder="🔍 检索项目路径或名称" />

    <div v-if="error" class="card err">{{ error }}</div>
    <div v-else-if="busy" class="card empty">加载中…</div>
    <div v-else-if="filtered.length === 0" class="card empty">
      暂无项目——在设备端用 opencode 打开过项目后会出现在这里
    </div>

    <transition-group name="fade" tag="div" class="list">
      <button v-for="p in filtered" :key="p.id" class="card proj" @click="openSessions(p)">
        <span v-if="p.vcs" class="vcs">{{ p.vcs }}</span>
        <div class="path">{{ p.name || p.canonical }}</div>
        <div class="sub">{{ fmtTime(p.canonical) }}</div>
      </button>
    </transition-group>
  </div>
</template>

<style scoped>
.page { max-width: 720px; margin: 0 auto; display: flex; flex-direction: column; gap: 12px; }
.head { display: flex; align-items: center; justify-content: space-between; gap: 12px; }
.title-block { display: flex; align-items: center; gap: 10px; min-width: 0; }
.title-block h1 { font-size: 24px; letter-spacing: -0.5px; }
.dev-tag { font-size: 13px; color: var(--text-2); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
.banner { background: var(--primary-weak); color: var(--primary); border-radius: var(--radius-m); padding: 10px 14px; font-size: 13px; }
.list { display: flex; flex-direction: column; gap: 10px; }
.proj { text-align: left; cursor: pointer; border: none; font: inherit; color: inherit; transition: transform 0.12s ease; }
.proj:active { transform: scale(0.99); }
.vcs { font-size: 11px; background: var(--surface-2); padding: 2px 8px; border-radius: 6px; color: var(--text-2); }
.path { font-weight: 600; margin-top: 6px; word-break: break-all; }
.sub { font-size: 13px; color: var(--text-2); margin-top: 2px; }
.empty { text-align: center; color: var(--text-2); padding: 28px; }
.err { color: var(--red); }
</style>
