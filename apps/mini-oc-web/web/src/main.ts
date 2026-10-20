import { createApp } from 'vue'
import { createPinia } from 'pinia'
import App from './App.vue'
import router from './router'
import './styles/main.css'

const app = createApp(App)
app.use(createPinia())
app.use(router)
app.mount('#app')

// 401 软跳转（api.ts 派发；由 router 处理，防止 location.href 刷新死循环）
window.addEventListener('mini-oc:unauthorized', () => {
  if (router.currentRoute.value.path !== '/login') {
    router.push('/login')
  }
})
