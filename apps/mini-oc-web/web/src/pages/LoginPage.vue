<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useAuthStore } from '../store'

const auth = useAuthStore()
const router = useRouter()
const key = ref('')
const busy = ref(false)
const showKey = ref(false)

async function submit() {
  if (!key.value.trim() || busy.value) return
  busy.value = true
  try {
    await auth.login(key.value.trim())
    router.push('/devices')
  } finally {
    busy.value = false
  }
}
</script>

<template>
  <div class="wrap">
    <div class="hero">
      <div class="logo">mini·oc</div>
      <p class="sub">你的 AI 编程工作站门户</p>
    </div>
    <div class="card form">
      <label>用户密钥</label>
      <div class="key-field">
        <input
          v-model="key"
          :type="showKey ? 'text' : 'password'"
          placeholder="32 位密钥"
          autocomplete="current-password"
          @keyup.enter="submit"
        />
        <button
          type="button"
          class="key-toggle"
          :aria-label="showKey ? '隐藏密钥' : '显示密钥'"
          :title="showKey ? '隐藏密钥' : '显示密钥'"
          @click="showKey = !showKey"
        >
          <!-- eye / eye-off 线性图标 -->
          <svg v-if="showKey" viewBox="0 0 24 24" width="18" height="18" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
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
      <p v-if="auth.error" class="error">{{ auth.error }}</p>
      <button class="btn" :disabled="busy || !key.trim()" @click="submit">
        {{ busy ? '登录中…' : '登录' }}
      </button>
      <a class="admin-link" href="/admin/login">管理员入口</a>
    </div>
  </div>
</template>

<style scoped>
.wrap { max-width: 400px; margin: 10vh auto 0; display: flex; flex-direction: column; gap: 24px; }
.hero { text-align: center; }
.logo { font-size: 42px; font-weight: 800; letter-spacing: -1px; color: var(--primary); }
.sub { color: var(--text-2); margin-top: 6px; }
.form { display: flex; flex-direction: column; gap: 12px; }
.form label { font-size: 14px; color: var(--text-2); }
.key-field { position: relative; }
.key-field input { padding-right: 44px; }
.key-toggle {
  position: absolute; right: 6px; top: 50%; transform: translateY(-50%);
  display: flex; align-items: center; justify-content: center;
  width: 34px; height: 34px; padding: 0;
  border: none; border-radius: 8px;
  background: transparent; color: var(--text-2);
  cursor: pointer; transition: background 0.12s ease, color 0.12s ease;
}
.key-toggle:hover { background: var(--surface-2); color: var(--text); }
.key-toggle:active { transform: translateY(-50%) scale(0.92); }
.key-toggle:focus-visible { outline: 2px solid var(--primary); outline-offset: 1px; }
.error { color: var(--red); font-size: 13px; }
.admin-link { text-align: center; font-size: 13px; color: var(--text-2); text-decoration: none; }
</style>
