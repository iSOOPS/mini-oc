<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { api, type AdminUser, type Device } from '../api'

const users = ref<AdminUser[]>([])
const busy = ref(false)
const error = ref('')

// 新增/编辑表单
const showForm = ref(false)
const editingId = ref<string | null>(null)
const formName = ref('')
const formDevices = ref<Partial<Device>[]>([])

// 密钥一次性展示面板
const keyPanel = ref<{ name: string; key: string } | null>(null)

onMounted(load)

async function load() {
  busy.value = true
  try {
    const r = await api.adminUsers()
    users.value = r.users
  } catch (e: any) {
    error.value = e?.message ?? '加载失败'
  } finally {
    busy.value = false
  }
}

function openCreate() {
  editingId.value = null
  formName.value = ''
  formDevices.value = [emptyDevice()]
  showForm.value = true
}

function openEdit(u: AdminUser) {
  editingId.value = u.id
  formName.value = u.name
  formDevices.value = u.devices.map((d) => ({ ...d }))
  showForm.value = true
}

function emptyDevice(): Partial<Device> {
  return { desc: '', name: '', port: 9464, pctype: 'macos', publicUrl: '', bound: false, deviceName: '' }
}

async function saveForm() {
  error.value = ''
  try {
    if (editingId.value) {
      await api.adminUpdateUser(editingId.value, {
        name: formName.value,
        devices: formDevices.value,
      })
    } else {
      const r = await api.adminCreateUser({
        name: formName.value,
        devices: formDevices.value,
      })
      keyPanel.value = { name: r.user.name, key: r.user.key }
    }
    showForm.value = false
    await load()
  } catch (e: any) {
    error.value = e?.message ?? '保存失败'
  }
}

async function regenerate(u: AdminUser) {
  if (!confirm(`重置 ${u.name} 的密钥？旧密钥将立即失效。`)) return
  const r = await api.adminRegenerateKey(u.id)
  keyPanel.value = { name: u.name, key: r.key }
  await load()
}

async function remove(u: AdminUser) {
  if (!confirm(`删除用户 ${u.name}（${u.id}）？不可恢复。`)) return
  await api.adminDeleteUser(u.id)
  await load()
}

// 穿透地址输入预处理：剥协议/路径（V1 peelTunnelInput 行为）
function peel(v: Partial<Device>) {
  const raw = (v.publicUrl ?? '').trim()
  const m = raw.match(/^(https?:\/\/)?([^/\s]+).*/i)
  v.publicUrl = m ? `${m[1] ?? 'https://'}${m[2]}` : raw
}
</script>

<template>
  <div class="page">
    <header class="head">
      <h1>用户管理</h1>
      <div class="actions">
        <button class="btn ghost small" @click="load">↻ 刷新</button>
        <button class="btn small" @click="openCreate">＋ 新增用户</button>
      </div>
    </header>

    <div v-if="keyPanel" class="card key-panel">
      <p><strong>{{ keyPanel.name }}</strong> 的密钥（仅此一次展示，请立即分发）：</p>
      <code class="key">{{ keyPanel.key }}</code>
      <button class="btn small" @click="keyPanel = null">我已保存</button>
    </div>

    <div v-if="error" class="card err">{{ error }}</div>

    <div v-if="showForm" class="card form">
      <h3>{{ editingId ? '编辑用户' : '新增用户' }}</h3>
      <label>用户名（字母数字 _ -）</label>
      <input v-model="formName" placeholder="如 samuel" />

      <h4>设备清单</h4>
      <div v-for="(d, i) in formDevices" :key="i" class="device-row">
        <div class="grid">
          <div><label>描述</label><input v-model="d.desc" placeholder="工作电脑" /></div>
          <div><label>服务名（rathole，小写-）</label><input v-model="d.name" placeholder="macbook-pro" /></div>
          <div>
            <label>平台</label>
            <select v-model="d.pctype"><option>macos</option><option>windows</option></select>
          </div>
          <div><label>opencode 端口</label><input v-model.number="d.port" type="number" /></div>
          <div class="wide"><label>穿透地址（自动剥协议）</label><input v-model="d.publicUrl" @blur="peel(d)" placeholder="oc.example.com 或 http://1.2.3.4:9101" /></div>
        </div>
        <button v-if="formDevices.length > 1" class="btn ghost small" @click="formDevices.splice(i, 1)">移除</button>
      </div>
      <button class="btn ghost small" @click="formDevices.push(emptyDevice())">＋ 添加设备</button>

      <div class="form-actions">
        <button class="btn ghost" @click="showForm = false">取消</button>
        <button class="btn" :disabled="!formName.trim()" @click="saveForm">保存</button>
      </div>
    </div>

    <div class="card user" v-for="u in users" :key="u.id">
      <div class="uhead">
        <div>
          <strong>{{ u.name }}</strong>
          <span class="uid">#{{ u.id }}</span>
        </div>
        <div class="uactions">
          <button class="btn ghost small" @click="openEdit(u)">编辑</button>
          <button class="btn ghost small" @click="regenerate(u)">重置密钥</button>
          <button class="btn danger small" @click="remove(u)">删除</button>
        </div>
      </div>
      <p class="devs">
        设备（{{ u.devices.length }}）：{{ u.devices.map((d) => `${d.name}:${d.port}(${d.pctype})`).join('、') || '无' }}
      </p>
    </div>

    <div v-if="!busy && users.length === 0" class="card empty">暂无用户</div>
  </div>
</template>

<style scoped>
.page { max-width: 760px; margin: 0 auto; display: flex; flex-direction: column; gap: 14px; }
.head { display: flex; align-items: center; justify-content: space-between; }
.head h1 { font-size: 26px; letter-spacing: -0.5px; }
.actions { display: flex; gap: 8px; }
.uid { color: var(--text-2); font-size: 13px; margin-left: 6px; }
.uhead { display: flex; justify-content: space-between; align-items: center; gap: 10px; flex-wrap: wrap; }
.uactions { display: flex; gap: 6px; }
.devs { margin-top: 8px; font-size: 13px; color: var(--text-2); word-break: break-all; }
.key-panel { display: flex; flex-direction: column; gap: 10px; align-items: flex-start; }
.key { align-self: stretch; background: var(--surface-2); padding: 12px; border-radius: 10px; font-size: 15px; word-break: break-all; }
.form { display: flex; flex-direction: column; gap: 10px; }
.form h3 { font-size: 18px; }
.form h4 { font-size: 15px; margin-top: 8px; }
.form label { font-size: 13px; color: var(--text-2); display: block; margin-bottom: 4px; }
.device-row { border-top: 1px dashed var(--surface-2); padding-top: 12px; display: flex; flex-direction: column; gap: 8px; }
.grid { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
.grid .wide { grid-column: 1 / -1; }
.form-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 6px; }
.empty { text-align: center; color: var(--text-2); padding: 28px; }
.err { color: var(--red); }
</style>
