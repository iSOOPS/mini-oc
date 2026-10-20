<script setup lang="ts">
import { computed } from 'vue'
import { useRoute } from 'vue-router'
import { useAuthStore } from './store'

const route = useRoute()
const auth = useAuthStore()

// 管理页独立布局（无底部 Tab）
const isAdmin = computed(() => route.path.startsWith('/admin'))
const isAuthPage = computed(() => route.path === '/login' || route.path === '/admin/login')

const tabs = [
  { path: '/devices', label: '设备', icon: '🖥' },
  { path: '/projects', label: '项目', icon: '📁' },
  { path: '/sessions', label: '会话', icon: '💬' },
]
</script>

<template>
  <div class="shell" :class="{ auth: isAuthPage || isAdmin }">
    <main class="content">
      <router-view v-slot="{ Component }">
        <transition name="fade" mode="out-in">
          <component :is="Component" />
        </transition>
      </router-view>
    </main>

    <template v-if="!isAuthPage && !isAdmin">
      <!-- 底部 TabBar（移动优先）；≥768px 时变侧栏 -->
      <nav class="tabbar">
        <router-link v-for="t in tabs" :key="t.path" :to="t.path" class="tab">
          <span class="icon">{{ t.icon }}</span>
          <span>{{ t.label }}</span>
        </router-link>
        <a class="tab" @click.prevent="auth.logout().then(() => $router.push('/login'))">
          <span class="icon">🚪</span><span>退出</span>
        </a>
      </nav>
    </template>
  </div>
</template>

<style scoped>
.shell { height: 100%; display: flex; flex-direction: column; }
.content { flex: 1; overflow-y: auto; padding: 16px; padding-bottom: 76px; }
.shell.auth .content { padding-bottom: 16px; }

.tabbar {
  position: fixed; bottom: 0; left: 0; right: 0;
  display: flex; background: var(--surface);
  border-top: 1px solid var(--surface-2);
  padding: 6px 8px calc(6px + env(safe-area-inset-bottom));
}
.tab {
  flex: 1; display: flex; flex-direction: column; align-items: center; gap: 2px;
  text-decoration: none; color: var(--text-2); font-size: 11px; padding: 6px 0;
  border-radius: 12px; transition: color 0.15s ease;
}
.tab .icon { font-size: 20px; }
.tab.router-link-active { color: var(--primary); }

@media (min-width: 768px) {
  .shell { flex-direction: row; }
  .content { flex: 1; padding: 24px; padding-bottom: 24px; }
  .tabbar {
    position: static; flex-direction: column; width: 200px;
    border-top: none; border-right: 1px solid var(--surface-2);
    padding: 20px 12px; gap: 6px; justify-content: flex-start;
  }
  .tab { flex: none; flex-direction: row; justify-content: flex-start; gap: 12px; padding: 12px 16px; font-size: 15px; }
  .tab .icon { font-size: 17px; }
}
</style>
