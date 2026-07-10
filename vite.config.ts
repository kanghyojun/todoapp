import { defineConfig } from "vitest/config";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [solid()],
  server: {
    host: "127.0.0.1",
    port: 2471,
    strictPort: true,
  },
  test: {
    environment: "node",
  },
});
