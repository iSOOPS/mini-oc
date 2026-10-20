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
router.beforeEach((to) => {
  const auth = useAuthStore()
  if (to.meta.public) return true
  if (to.meta.admin) {
    if (sessionStorage.getItem('mini_oc_admin') === '1') return true
    return { path: '/admin/login' }
  }
  if (!auth.isLoggedIn) return { path: '/login' }
  return true
})

export default router
