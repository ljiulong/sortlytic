import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

const windowConfig = JSON.parse(
  readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'),
)
const globalStyles = readFileSync(new URL('../src/index.css', import.meta.url), 'utf8')
const nativeWindowSource = readFileSync(
  new URL('../src-tauri/src/native_window.rs', import.meta.url),
  'utf8',
)

describe('macOS 应用外壳', () => {
  it('关闭原生外框阴影且不叠加整窗伪元素', () => {
    expect(windowConfig.app.windows[0]).toMatchObject({
      decorations: false,
      shadow: false,
    })
    expect(globalStyles).not.toContain('.app-shell::before')

    const shellRule = globalStyles.match(/\.app-shell\s*\{([^}]*)\}/)?.[1]

    expect(shellRule).toBeDefined()
    expect(shellRule).toMatch(/border:\s*(?:0|none)\s*;/)
    expect(shellRule).toMatch(/box-shadow:\s*none\s*;/)
  })

  it('仅清空原生窗口底层以显示真实圆角', () => {
    expect(windowConfig.app.windows[0]).toMatchObject({
      transparent: false,
    })
    expect(nativeWindowSource).toContain('b"clearColor\\0"')
    expect(nativeWindowSource).toContain('b"setOpaque:\\0", false')
    expect(nativeWindowSource).toContain('b"setBackgroundColor:\\0"')
  })
})
