<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { api } from '../api'

const router = useRouter()
const password = ref('')
const error = ref('')
const busy = ref(false)
const showPassword = ref(false)

async function submit() {
  if (!password.value || busy.value) return
  busy.value = true
  error.value = ''
  try {
    await api.adminLogin(password.value.trim())
    sessionStorage.setItem('mini_oc_admin', '1')
    router.push('/admin')
  } catch (e: any) {
    error.value = e?.message ?? '密码错误'
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="wrap card">
    <h2>管理员登录</h2>
    <div class="pwd-field">
      <input
        v-model="password"
        :type="showPassword ? 'text' : 'password'"
        placeholder="管理员密码"
        autocomplete="current-password"
        @keyup.enter="submit"
      />
      <button
        type="button"
        class="pwd-toggle"
        :aria-label="showPassword ? '隐藏密码' : '显示密码'"
        :title="showPassword ? '隐藏密码' : '显示密码'"
        @click="showPassword = !showPassword"
      >
        <!-- eye / eye-off 线性图标 -->
        <svg v-if="showPassword" viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94" />
          <path d="M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19" />
          <path d="M14.12 14.12A3 3 0 1 1 9.88 9.88" />
          <line x1="1" y1="1" x2="23" y2="23" />
        </svg>
        <svg v-else viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
          <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
          <circle cx="12" cy="12" r="3" />
        </svg>
      </button>
    </div>
    <p v-if="error" class="error">{{ error }}</p>
    <button class="btn" :disabled="busy || !password" @click="submit">{{ busy ? '验证中…' : '登录' }}</button>
    <a class="back" href="/login">← 返回用户登录</a>
  </div>
</template>

<style scoped>
.wrap { max-width: 380px; margin: 10vh auto 0; display: flex; flex-direction: column; gap: 12px; }
h2 { font-size: 22px; }
.pwd-field { position: relative; }
.pwd-field input { padding-right: 44px; }
.pwd-toggle {
  position: absolute; right: 6px; top: 50%; transform: translateY(-50%);
  display: flex; align-items: center; justify-content: center;
  width: 34px; height: 34px; padding: 0;
  border: none; border-radius: 8px;
  background: transparent; color: var(--text-2);
  cursor: pointer; transition: background 0.12s ease, color 0.12s ease;
}
.pwd-toggle:hover { background: var(--surface-2); color: var(--text); }
.pwd-toggle:active { transform: translateY(-50%) scale(0.92); }
.pwd-toggle:focus-visible { outline: 2px solid var(--primary); outline-offset: 1px; }
.error { color: var(--red); font-size: 13px; }
.back { text-align: center; font-size: 13px; color: var(--text-2); }
</style>
