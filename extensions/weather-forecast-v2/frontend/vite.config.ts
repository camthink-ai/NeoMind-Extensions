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
      name: 'WeatherForecastV2Components',
      fileName: 'weather-forecast-v2-components',
      formats: ['umd']
    },
    // Don't externalize React - bundle it for cross-environment compatibility
    rollupOptions: {
      output: {
        exports: 'named'
      }
    },
    outDir: 'dist',
    emptyOutDir: true
  }
})
