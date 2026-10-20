<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { useAuthStore } from '../store'

const auth = useAuthStore()
const router = useRouter()
const key = ref('')
const busy = ref(false)

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
      <input v-model="key" type="password" placeholder="32 位密钥" @keyup.enter="submit" />
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
.error { color: var(--red); font-size: 13px; }
.admin-link { text-align: center; font-size: 13px; color: var(--text-2); text-decoration: none; }
</style>
