import { createRouter, createWebHistory } from 'vue-router'
import { useAuthStore } from './store'

const router = createRouter({
  history: createWebHistory(),
  routes: [
    { path: '/login', component: () => import('./pages/LoginPage.vue'), meta: { public: true } },
    { path: '/', redirect: '/devices' },
    { path: '/devices', component: () => import('./pages/DevicesPage.vue') },
    { path: '/projects', component: () => import('./pages/ProjectsPage.vue') },
    { path: '/sessions', component: () => import('./pages/SessionsPage.vue') },
    { path: '/admin/login', component: () => import('./pages/AdminLoginPage.vue'), meta: { public: true } },
    { path: '/admin', component: () => import('./pages/AdminPage.vue'), meta: { admin: true } },
    { path: '/:pathMatch(.*)*', redirect: '/devices' },
  ],
})

// 三级守卫（public / 需登录 / admin）
//
// 刷新恢复（I 修复）：Pinia 是内存态，刷新即丢；cookie（HttpOnly，7 天）仍在。
// 首次导航到需登录页面时先静默调 /api/me 回源验证：
//   - 成功 → 恢复 store 登录态，放行（刷新不再退回登录页）
//   - 401 → 确实未登录/过期，跳登录页
// 回源只在每次冷启动做一次（restoreDone），之后的导航零开销走内存判断。
let restoreDone = false

router.beforeEach(async (to) => {
  const auth = useAuthStore()
  if (to.meta.public) return true
  if (to.meta.admin) {
    if (sessionStorage.getItem('mini_oc_admin') === '1') return true
    return { path: '/admin/login' }
  }
  if (!auth.isLoggedIn && !restoreDone) {
    restoreDone = true
    try {
      await auth.probe() // cookie 有效 → 恢复 user/devices
    } catch {
      return { path: '/login' } // 401：未登录或会话过期
    }
  }
  if (!auth.isLoggedIn) return { path: '/login' }
  return true
})

export default router
