import { defineStore } from 'pinia'
import { api, type Device, type Me } from './api'

export const useAuthStore = defineStore('auth', {
  state: () => ({
    user: null as Me['user'] | null,
    devices: [] as Device[],
    loading: false,
    error: '' as string,
  }),
  getters: {
    isLoggedIn: (s) => s.user !== null,
    onlineDevices: (s) => s.devices.filter((d) => d.status === 'online'),
  },
  actions: {
    async login(key: string) {
      this.error = ''
      try {
        const u = await api.login(key)
        this.user = u
        await this.probe()
      } catch (e: any) {
        this.error = e?.message ?? '登录失败'
        throw e
      }
    },
    async probe(signal?: AbortSignal) {
      this.loading = true
      try {
        const me = await api.me(signal)
        this.user = me.user
        this.devices = me.devices
      } finally {
        this.loading = false
      }
    },
    async logout() {
      try { await api.logout() } catch { /* 忽略 */ }
      this.user = null
      this.devices = []
    },
  },
})
