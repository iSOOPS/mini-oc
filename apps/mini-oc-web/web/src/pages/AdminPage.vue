<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { api, type AdminDevice, type AdminUser, type PendingDevice } from '../api'

const users = ref<AdminUser[]>([])
const busy = ref(false)
const error = ref('')

// 新增/编辑表单
const showForm = ref(false)
const editingId = ref<string | null>(null)
const formName = ref('')

// 编辑态下的设备编辑草稿（拷贝自用户数据，保存时按白名单字段提交）
const editDevices = ref<AdminDevice[]>([])

// 待审核折叠区展开状态（key：用户 id）
const expandedPending = ref<Record<string, boolean>>({})

// 「同意」小表单状态（key：pendingDevice.deviceId）
const approveForm = ref<Record<string, { open: boolean; name: string; publicUrl: string }>>({})

// 停用/启用进行中的设备（key：用户id:设备id或名），防连点并发
const toggling = ref<Record<string, boolean>>({})

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
  showForm.value = true
}

function openEdit(u: AdminUser) {
  editingId.value = u.id
  formName.value = u.name
  editDevices.value = u.devices.map((d) => ({ ...d }))
  showForm.value = true
}

async function saveForm() {
  error.value = ''
  try {
    if (editingId.value) {
      await api.adminUpdateUser(editingId.value, {
        name: formName.value,
        devices: editDevices.value.map((d) => ({
          deviceId: d.deviceId,
          name: d.name,
          publicUrl: d.publicUrl,
          enabled: d.enabled,
        })),
      })
    } else {
      const r = await api.adminCreateUser({ name: formName.value })
      keyPanel.value = { name: r.user.name, key: r.user.key }
    }
    showForm.value = false
    await load()
  } catch (e: any) {
    error.value = e?.message ?? '保存失败'
  }
}

// 编辑表单内的启用开关（仅改草稿，保存时统一提交）
function toggleEditDevice(d: AdminDevice) {
  d.enabled = !d.enabled
}

// 请求体同时带 deviceId 与 name：deviceId 非空时后端按 id 定位；
// 为空时（存量未回填设备）后端回退按 name 定位，否则必 404
async function toggleEnabled(uid: string, d: AdminDevice) {
  const k = `${uid}:${d.deviceId || d.name}`
  if (toggling.value[k]) return
  toggling.value[k] = true
  error.value = ''
  try {
    await api.adminUpdateUser(uid, {
      devices: [{ deviceId: d.deviceId, name: d.name, enabled: !d.enabled }],
    })
    await load()
  } catch (e: any) {
    error.value = e?.message ?? '操作失败'
  } finally {
    toggling.value[k] = false
  }
}

async function removeDevice(u: AdminUser, d: AdminDevice) {
  if (!confirm(`移除设备 ${d.name}（${d.desc || d.deviceName}）？不可恢复。`)) return
  error.value = ''
  try {
    await api.adminDeleteDevice(u.id, d.deviceId)
    await load()
  } catch (e: any) {
    error.value = e?.message ?? '移除失败'
  }
}

function togglePending(u: AdminUser) {
  expandedPending.value[u.id] = !expandedPending.value[u.id]
}

function openApproveForm(p: PendingDevice) {
  const f = approveForm.value[p.deviceId]
  // 已打开时不重置，避免再次点击清空已填内容
  if (f?.open) return
  approveForm.value[p.deviceId] = { open: true, name: f?.name ?? '', publicUrl: f?.publicUrl ?? '' }
}

function closeApproveForm(p: PendingDevice) {
  delete approveForm.value[p.deviceId]
}

async function approvePending(u: AdminUser, p: PendingDevice) {
  const f = approveForm.value[p.deviceId]
  if (!f || !f.name.trim() || !f.publicUrl.trim()) return
  error.value = ''
  try {
    await api.adminApproveDevice(u.id, p.deviceId, {
      name: f.name.trim(),
      publicUrl: f.publicUrl.trim(),
    })
    closeApproveForm(p)
    await load()
  } catch (e: any) {
    error.value = e?.message ?? '同意失败'
  }
}

async function rejectPending(u: AdminUser, p: PendingDevice) {
  if (!confirm(`拒绝设备 ${p.deviceName}（${p.desc || '无描述'}）的上报？`)) return
  error.value = ''
  try {
    await api.adminRejectDevice(u.id, p.deviceId)
    await load()
  } catch (e: any) {
    error.value = e?.message ?? '拒绝失败'
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
function peelStr(v: string): string {
  const raw = (v ?? '').trim()
  const m = raw.match(/^(https?:\/\/)?([^/\s]+).*/i)
  return m ? `${m[1] ?? 'https://'}${m[2]}` : raw
}

function peel(d: { publicUrl: string }) {
  d.publicUrl = peelStr(d.publicUrl)
}

function approveField(p: PendingDevice, field: 'name' | 'publicUrl'): string {
  return approveForm.value[p.deviceId]?.[field] ?? ''
}

function setApproveField(p: PendingDevice, field: 'name' | 'publicUrl', v: string) {
  const f = approveForm.value[p.deviceId]
  if (f) f[field] = v
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

      <template v-if="editingId">
        <h4>设备清单（只读项由客户端上报维护）</h4>
        <div v-for="d in editDevices" :key="d.deviceId || d.name" class="device-row">
          <div class="grid">
            <div><label>描述（客户端上报）</label><input :value="d.desc" readonly /></div>
            <div><label>服务名</label><input v-model="d.name" placeholder="macbook-pro" /></div>
            <div><label>平台（客户端上报）</label><input :value="d.pctype" readonly /></div>
            <div><label>端口（客户端上报）</label><input :value="d.port" readonly /></div>
            <div><label>主机名（客户端上报）</label><input :value="d.deviceName" readonly /></div>
            <div><label>绑定状态</label><input :value="d.bound ? '已绑定' : '未绑定'" readonly /></div>
            <div class="wide"><label>穿透地址（自动剥协议）</label><input v-model="d.publicUrl" @blur="peel(d)" placeholder="oc.example.com 或 http://1.2.3.4:9101" /></div>
          </div>
          <div class="row-actions">
            <button class="btn ghost small" @click="toggleEditDevice(d)">{{ d.enabled ? '停用' : '启用' }}</button>
          </div>
        </div>
      </template>

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

      <div class="device-cards">
        <div v-for="d in u.devices" :key="d.deviceId || d.name" class="device-card">
          <div class="drow1">
            <strong>{{ d.desc }}</strong>
            <span class="dmeta">{{ d.name }}:{{ d.port }}（{{ d.pctype }}）· {{ d.deviceName || '未知主机' }} · {{ d.bound ? '已绑定' : '未绑定' }}</span>
            <span v-if="!d.enabled" class="badge disabled">已停用</span>
            <span v-else-if="d.bound" class="badge bound">已绑定</span>
          </div>
          <div class="dactions">
            <button class="btn ghost small" :disabled="toggling[`${u.id}:${d.deviceId || d.name}`]" @click="toggleEnabled(u.id, d)">{{ d.enabled ? '停用' : '启用' }}</button>
            <button v-if="!d.enabled" class="btn danger small" @click="removeDevice(u, d)">移除</button>
          </div>
        </div>
      </div>

      <div v-if="u.pendingDevices.length > 0" class="pending">
        <button class="pending-toggle" @click="togglePending(u)">
          待审核设备 ({{ u.pendingDevices.length }}) {{ expandedPending[u.id] ? '▾' : '▸' }}
        </button>
        <div v-if="expandedPending[u.id]" class="pending-list">
          <div v-for="p in u.pendingDevices" :key="p.deviceId" class="pending-item">
            <div class="pinfo">
              <strong>{{ p.desc || p.deviceName }}</strong>
              <span class="dmeta">{{ p.port }}（{{ p.pctype }}）· {{ p.deviceName }} · 上报于 {{ p.reportedAt }}</span>
            </div>
            <div class="pactions">
              <button class="btn small" @click="openApproveForm(p)">同意</button>
              <button class="btn ghost small" @click="rejectPending(u, p)">拒绝</button>
            </div>
            <div v-if="approveForm[p.deviceId]?.open" class="approve-form">
              <div class="grid">
                <div>
                  <label>服务名（必填）</label>
                  <input :value="approveField(p, 'name')" @input="setApproveField(p, 'name', ($event.target as HTMLInputElement).value)" placeholder="macbook-pro" />
                </div>
                <div>
                  <label>穿透地址（必填，自动剥协议）</label>
                  <input :value="approveField(p, 'publicUrl')" @blur="setApproveField(p, 'publicUrl', peelStr(approveField(p, 'publicUrl')))" @input="setApproveField(p, 'publicUrl', ($event.target as HTMLInputElement).value)" placeholder="oc.example.com" />
                </div>
              </div>
              <div class="form-actions">
                <button class="btn ghost small" @click="closeApproveForm(p)">取消</button>
                <button class="btn small" :disabled="!approveField(p, 'name').trim() || !approveField(p, 'publicUrl').trim()" @click="approvePending(u, p)">确认同意</button>
              </div>
            </div>
          </div>
        </div>
      </div>
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
.row-actions { display: flex; gap: 8px; justify-content: flex-end; }
.form-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 6px; }
.device-cards { display: flex; flex-direction: column; gap: 8px; margin-top: 10px; }
.device-card { border: 1px solid var(--surface-2); border-radius: 10px; padding: 10px 12px; display: flex; justify-content: space-between; align-items: center; gap: 10px; flex-wrap: wrap; }
.device-card .drow1 { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
.dmeta { font-size: 12px; color: var(--text-2); word-break: break-all; }
.dactions { display: flex; gap: 6px; margin-left: auto; }
.badge { font-size: 12px; font-weight: 600; padding: 2px 8px; border-radius: 6px; align-self: flex-start; }
.badge.disabled { background: var(--surface-2); color: var(--text-2); }
.badge.bound { color: var(--green); }
.pending { margin-top: 10px; }
.pending-toggle { background: none; border: none; padding: 0; font-size: 14px; font-weight: 600; color: var(--orange); cursor: pointer; }
.pending-list { display: flex; flex-direction: column; gap: 8px; margin-top: 8px; }
.pending-item { border: 1px dashed var(--surface-2); border-radius: 10px; padding: 10px 12px; display: flex; flex-direction: column; gap: 8px; }
.pending-item .pinfo { display: flex; flex-direction: column; gap: 2px; }
.pending-item .pactions { display: flex; gap: 6px; }
.approve-form { border-top: 1px dashed var(--surface-2); padding-top: 10px; display: flex; flex-direction: column; gap: 8px; }
.empty { text-align: center; color: var(--text-2); padding: 28px; }
.err { color: var(--red); }
</style>
