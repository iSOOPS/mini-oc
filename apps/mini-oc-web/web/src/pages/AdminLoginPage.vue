<script setup lang="ts">
import { ref } from 'vue'
import { useRouter } from 'vue-router'
import { api } from '../api'

const router = useRouter()
const password = ref('')
const error = ref('')
const busy = ref(false)

async function submit() {
  if (!password.value || busy.value) return
  busy.value = true
  error.value = ''
  try {
    await api.adminLogin(password.value)
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
    <input v-model="password" type="password" placeholder="管理员密码" @keyup.enter="submit" />
    <p v-if="error" class="error">{{ error }}</p>
    <button class="btn" :disabled="busy || !password" @click="submit">{{ busy ? '验证中…' : '登录' }}</button>
    <a class="back" href="/login">← 返回用户登录</a>
  </div>
</template>

<style scoped>
.wrap { max-width: 380px; margin: 10vh auto 0; display: flex; flex-direction: column; gap: 12px; }
h2 { font-size: 22px; }
.error { color: var(--red); font-size: 13px; }
.back { text-align: center; font-size: 13px; color: var(--text-2); }
</style>
