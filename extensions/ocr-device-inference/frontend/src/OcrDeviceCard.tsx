import React from 'react'

export interface OcrDeviceCardProps {
  executeCommand?: (command: string, args: Record<string, unknown>) => Promise<unknown>
  config?: Record<string, unknown>
}

export const OcrDeviceCard: React.FC<OcrDeviceCardProps> = ({
  executeCommand,
  config = {}
}) => {
  return (
    <div style={{ padding: '16px', fontFamily: 'sans-serif' }}>
      <h3>OCR识别</h3>
      <p>OCR识别组件正在开发中...</p>
    </div>
  )
}

export default { OcrDeviceCard }
