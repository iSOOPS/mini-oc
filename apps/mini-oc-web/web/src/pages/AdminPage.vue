<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { useRouter } from 'vue-router'
import { api, type AdminDevice, type AdminUser, type PendingDevice } from '../api'

const router = useRouter()
const users = ref<AdminUser[]>([])
const busy = ref(false)
const error = ref('')

// 新增用户表单
const showForm = ref(false)
const formName = ref('')

// 设备编辑态：仅一台设备的草稿（拷贝自列表），保存时按白名单字段单独提交，
// 不影响其他设备（后端 devices 为逐条定位合并语义）。
// matchName 记录打开编辑时的原始服务名：deviceId 为空的存量设备改名后，
// 后端仍能按原名定位（name 已是新值，不能兼任定位键）。
const editingDevice = ref<{ userId: string; device: AdminDevice; matchName: string } | null>(null)

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
  formName.value = ''
  showForm.value = true
}

// 打开单设备编辑：拷贝草稿，记录原始服务名作定位键
function openDeviceEdit(u: AdminUser, d: AdminDevice) {
  editingDevice.value = { userId: u.id, device: { ...d }, matchName: d.name }
}

function closeDeviceEdit() {
  editingDevice.value = null
}

async function saveDeviceEdit() {
  const ed = editingDevice.value
  if (!ed) return
  error.value = ''
  try {
    await api.adminUpdateUser(ed.userId, {
      devices: [
        {
          deviceId: ed.device.deviceId,
          matchName: ed.matchName,
          name: ed.device.name,
          publicUrl: ed.device.publicUrl,
          enabled: ed.device.enabled,
        },
      ],
    })
    editingDevice.value = null
    await load()
  } catch (e: any) {
    error.value = e?.message ?? '保存失败'
  }
}

async function saveForm() {
  error.value = ''
  try {
    const r = await api.adminCreateUser({ name: formName.value })
    keyPanel.value = { name: r.user.name, key: r.user.key }
    showForm.value = false
    await load()
  } catch (e: any) {
    error.value = e?.message ?? '保存失败'
  }
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
    // 空 deviceId（存量设备）以 name 作 did：后端先按 device_id、未命中按
    // name 回退定位；空串会拼出残缺 URL 必 404
    await api.adminDeleteDevice(u.id, d.deviceId || d.name)
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

// 退出管理会话：清 cookie（服务端 Max-Age=0）+ 本地标记 + 回登录页
async function logout() {
  try {
    await api.adminLogout()
  } catch {
    /* 本地清理优先，网络失败不阻塞退出 */
  }
  sessionStorage.removeItem('mini_oc_admin')
  router.push('/admin/login')
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
        <button class="btn ghost small" @click="logout">退出</button>
      </div>
    </header>

    <div v-if="keyPanel" class="card key-panel">
      <p><strong>{{ keyPanel.name }}</strong> 的密钥（仅此一次展示，请立即分发）：</p>
      <code class="key">{{ keyPanel.key }}</code>
      <button class="btn small" @click="keyPanel = null">我已保存</button>
    </div>

    <div v-if="error" class="card err">{{ error }}</div>

    <div v-if="showForm" class="card form">
      <h3>新增用户</h3>
      <label>用户名（字母数字 _ -）</label>
      <input v-model="formName" placeholder="如 samuel" />

      <div class="form-actions">
        <button class="btn ghost" @click="showForm = false">取消</button>
        <button class="btn" :disabled="!formName.trim()" @click="saveForm">保存</button>
      </div>
    </div>

    <!-- 单设备编辑：进入后隐藏用户/设备列表，仅编辑选中设备 -->
    <div v-if="editingDevice" class="card form device-edit">
      <header class="de-head">
        <h3>编辑设备</h3>
        <span v-if="!editingDevice.device.enabled" class="badge disabled">已停用</span>
      </header>

      <!-- 只读上报信息：客户端上报维护，不可修改 -->
      <div class="ro-grid">
        <div class="ro-item">
          <span class="ro-label">描述</span>
          <span class="ro-value">{{ editingDevice.device.desc || '—' }}</span>
        </div>
        <div class="ro-item">
          <span class="ro-label">平台</span>
          <span class="ro-value">{{ editingDevice.device.pctype }}</span>
        </div>
        <div class="ro-item">
          <span class="ro-label">端口</span>
          <span class="ro-value">{{ editingDevice.device.port }}</span>
        </div>
        <div class="ro-item">
          <span class="ro-label">主机名</span>
          <span class="ro-value">{{ editingDevice.device.deviceName || '—' }}</span>
        </div>
        <div class="ro-item">
          <span class="ro-label">绑定状态</span>
          <span class="ro-value" :class="{ ok: editingDevice.device.bound }">
            {{ editingDevice.device.bound ? '已绑定' : '未绑定' }}
          </span>
        </div>
      </div>

      <!-- 可编辑字段 -->
      <div class="edit-grid">
        <div>
          <label>服务名（rathole service 名）</label>
          <input v-model="editingDevice.device.name" placeholder="macbook-pro" />
        </div>
        <div>
          <label>穿透地址（自动剥协议）</label>
          <input v-model="editingDevice.device.publicUrl" @blur="peel(editingDevice.device)" placeholder="oc.example.com 或 http://1.2.3.4:9101" />
        </div>
      </div>

      <div class="form-actions">
        <button class="btn ghost" @click="closeDeviceEdit">取消</button>
        <button class="btn" :disabled="!editingDevice.device.name.trim() || !editingDevice.device.publicUrl.trim()" @click="saveDeviceEdit">保存</button>
      </div>
    </div>

    <template v-if="!editingDevice && !showForm">
    <div class="card user" v-for="u in users" :key="u.id">
      <div class="uhead">
        <div>
          <strong>{{ u.name }}</strong>
          <span class="uid">#{{ u.id }}</span>
        </div>
        <div class="uactions">
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
            <button class="btn ghost small" @click="openDeviceEdit(u, d)">编辑</button>
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

    <div v-if="!busy && users.length === 0 && !editingDevice && !showForm" class="card empty">暂无用户</div>
    </template>
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
.form label { font-size: 13px; color: var(--text-2); display: block; margin-bottom: 4px; }
.grid { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
.form-actions { display: flex; justify-content: flex-end; gap: 8px; margin-top: 6px; }

/* 单设备编辑布局：只读上报信息块 + 可编辑字段 */
.device-edit .de-head { display: flex; align-items: center; gap: 10px; }
.ro-grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(140px, 1fr)); gap: 8px; padding: 12px; background: var(--surface-2); border-radius: 10px; }
.ro-item { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
.ro-label { font-size: 12px; color: var(--text-2); }
.ro-value { font-size: 14px; font-weight: 500; word-break: break-all; }
.ro-value.ok { color: var(--green); }
.edit-grid { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; }
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
