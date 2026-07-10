import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';

export default defineConfig({
  plugins: [react()],
  test: {
    environment: 'happy-dom',
    exclude: ['**/node_modules/**', '**/dist/**', '**/src-tauri/**', '**/.claude/**'],
    globals: false,
    setupFiles: ['./src/test/setup.ts'],
  },
});
