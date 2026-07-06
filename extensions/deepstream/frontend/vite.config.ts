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
      name: 'DeepStreamComponents',
      fileName: (format) => format === 'umd' ? 'deepstream-components.umd.js' : 'deepstream-components.umd.cjs',
      formats: ['umd', 'cjs']
    },
    // Externalize React - use host app's React via window globals
    rollupOptions: {
      external: ['react', 'react-dom'],
      output: {
        exports: 'named',
        globals: {
          react: 'React',
          'react-dom': 'ReactDOM',
        },
      },
    },
    outDir: 'dist',
    emptyOutDir: true
  }
})
