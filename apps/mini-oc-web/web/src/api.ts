// API 封装：fetch + 401 软跳转约定 + 全部响应类型
export interface ApiError { code: string; message: string }

async function request<T>(path: string, init?: RequestInit, opts?: { on401?: 'redirect' | 'passthrough' }): Promise<T> {
  const resp = await fetch(path, {
    headers: { 'Content-Type': 'application/json' },
    ...init,
  })
  if (resp.status === 401) {
    if (opts?.on401 === 'passthrough') {
      // 登录接口：透传后端真实错误（"密码错误"），不触发软跳转
      const body = await resp.json().catch(() => ({ error: { code: 'unauthorized', message: 'unauthorized' } }))
      throw body.error as ApiError
    }
    // 软跳转：交给 router 守卫处理（V1 验证过的防刷新死循环模式）
    const err: ApiError = { code: 'unauthorized', message: '登录已过期' }
    window.dispatchEvent(new CustomEvent('mini-oc:unauthorized'))
    throw err
  }
  if (!resp.ok) {
    const body = await resp.json().catch(() => ({ error: { code: 'internal', message: `HTTP ${resp.status}` } }))
    throw body.error as ApiError
  }
  return resp.json() as Promise<T>
}

// —— 类型（与后端响应一致，camelCase）——
export type DeviceStatus = 'online' | 'offline' | 'auth_mismatch'

export interface Device {
  desc: string
  name: string
  port: number
  pctype: string
  deviceName: string
  bound: boolean
  publicUrl: string
  status?: DeviceStatus
  version?: string | null
}

export interface Me {
  user: { id: string; name: string }
  devices: Device[]
}

export interface Project {
  id: string
  canonical: string
  vcs?: string | null
  name?: string | null
}

export interface Session {
  id: string
  projectID: string
  title?: string | null
  directory: string
  time: { created: number; updated: number }
}

export interface DualSource<T> {
  source: 'live' | 'snapshot'
  fetchedAt: string | null
  data: T[]
}

export interface AdminDevice extends Device {
  enabled: boolean
  deviceId: string
}

export interface PendingDevice {
  deviceId: string
  desc: string
  port: number
  pctype: string
  deviceName: string
  reportedAt: string
}

export interface AdminUser {
  id: string
  name: string
  key: string
  devices: AdminDevice[]
  pendingDevices: PendingDevice[]
  createdAt: string
  updatedAt: string
}

// —— 认证 ——
export const api = {
  login: (key: string) =>
    request<{ id: string; name: string }>('/api/login', { method: 'POST', body: JSON.stringify({ key }) }, { on401: 'passthrough' }),
  logout: () => request<{ ok: boolean }>('/api/logout', { method: 'POST' }),
  me: (signal?: AbortSignal) =>
    request<Me>('/api/me', { signal }),
  meKey: () => request<{ key: string }>('/api/me/key'),
  meName: (name: string) =>
    request<{ ok: boolean }>('/api/me/name', { method: 'POST', body: JSON.stringify({ name }) }),

  deviceProjects: (name: string) =>
    request<DualSource<Project>>(`/api/devices/${name}/projects`),
  deviceSessions: (name: string, directory?: string) =>
    request<DualSource<Session>>(`/api/devices/${name}/sessions${directory ? `?directory=${encodeURIComponent(directory)}` : ''}`),
  deviceStatus: (name: string) =>
    request<{ name: string; status: DeviceStatus; version: string | null }>(`/api/device-status/${name}`),
  jump: (name: string, directory: string, session?: string) =>
    request<{ url: string; expiresIn: number }>(
      `/api/devices/${name}/jump?directory=${encodeURIComponent(directory)}${session ? `&session=${session}` : ''}&format=json`,
    ),

  adminLogin: (password: string) =>
    request<{ ok: boolean }>('/api/admin/login', { method: 'POST', body: JSON.stringify({ password }) }, { on401: 'passthrough' }),
  adminLogout: () => request<{ ok: boolean }>('/api/admin/logout', { method: 'POST' }),
  adminUsers: () => request<{ users: AdminUser[] }>('/api/admin/users'),
  adminCreateUser: (body: { name: string }) =>
    request<{ user: AdminUser }>('/api/admin/users', { method: 'POST', body: JSON.stringify(body) }),
  adminUpdateUser: (id: string, body: { name?: string; devices?: { deviceId: string; name?: string; publicUrl?: string; enabled?: boolean }[] }) =>
    request<{ user: AdminUser }>(`/api/admin/users/${id}`, { method: 'PUT', body: JSON.stringify(body) }),
  adminDeleteUser: (id: string) =>
    request<{ ok: boolean }>(`/api/admin/users/${id}`, { method: 'DELETE' }),
  adminRegenerateKey: (id: string) =>
    request<{ key: string }>(`/api/admin/users/${id}/regenerate-key`, { method: 'POST' }),
  adminApproveDevice: (uid: string, did: string, body: { name: string; publicUrl: string }) =>
    request<{ user: AdminUser }>(`/api/admin/users/${uid}/devices/${did}/approve`, { method: 'POST', body: JSON.stringify(body) }),
  adminRejectDevice: (uid: string, did: string) =>
    request<{ ok: boolean }>(`/api/admin/users/${uid}/devices/${did}/reject`, { method: 'DELETE' }),
  adminDeleteDevice: (uid: string, did: string) =>
    request<{ ok: boolean }>(`/api/admin/users/${uid}/devices/${did}`, { method: 'DELETE' }),
}
