import { createApp } from "vue";
import App from "./App.vue";
import { initTheme } from "./composables/useTheme";
import "./styles/theme.css";

// 先定主题再挂载，避免首帧用错配色
initTheme();
createApp(App).mount("#app");
