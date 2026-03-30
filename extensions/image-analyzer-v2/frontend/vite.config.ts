import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

export default defineConfig({
  plugins: [react()],
  define: {
    'process.env.NODE_ENV': JSON.stringify('production')
  },
  build: {
    lib: {
      entry: 'src/index.tsx',
      name: 'ImageAnalyzerV2Components',
      fileName: 'image-analyzer-v2-components',
      formats: ['umd']
    },
    // Don't externalize React - bundle it for cross-environment compatibility
    // (desktop app Tauri has global React, web browser doesn't)
    rollupOptions: {
      output: {
        exports: 'named'
      }
    },
    outDir: 'dist',
    emptyOutDir: true
  }
})
