import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'

const windowConfig = JSON.parse(
  readFileSync(new URL('../src-tauri/tauri.conf.json', import.meta.url), 'utf8'),
)
const globalStyles = readFileSync(new URL('../src/index.css', import.meta.url), 'utf8')
const appStyles = readFileSync(new URL('../src/App.css', import.meta.url), 'utf8')
const frontendAppSource = readFileSync(new URL('../src/App.tsx', import.meta.url), 'utf8')
const nativeWindowSource = readFileSync(
  new URL('../src-tauri/src/native_window.rs', import.meta.url),
  'utf8',
)
const appSource = readFileSync(new URL('../src-tauri/src/lib.rs', import.meta.url), 'utf8')

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

  it('原生圆角装饰初始化失败时仍继续启动应用', () => {
    expect(appSource).toContain('if let Some(main_window) = app.get_webview_window("main")')
    expect(appSource).toContain(
      'if let Err(error) = native_window::apply_native_window_corner_radius(&main_window)',
    )
    expect(appSource).not.toContain('.map_err(std::io::Error::other)?')
  })

  it('固定公共顶栏并只让页面正文滚动', () => {
    expect(frontendAppSource).toContain('<div className="workspace-scroll">')

    const workspaceRule = appStyles.match(/\.workspace\s*\{([^}]*)\}/)?.[1]
    const topbarRule = appStyles.match(/\.topbar\s*\{([^}]*)\}/)?.[1]
    const scrollRule = appStyles.match(/\.workspace-scroll\s*\{([^}]*)\}/)?.[1]

    expect(workspaceRule).toBeDefined()
    expect(workspaceRule).toMatch(/display:\s*flex\s*;/)
    expect(workspaceRule).toMatch(/flex-direction:\s*column\s*;/)
    expect(workspaceRule).toMatch(/overflow:\s*hidden\s*;/)
    expect(topbarRule).toMatch(/flex:\s*0\s+0\s+auto\s*;/)
    expect(scrollRule).toMatch(/flex:\s*1\s+1\s+auto\s*;/)
    expect(scrollRule).toMatch(/min-height:\s*0\s*;/)
    expect(scrollRule).toMatch(/overflow-y:\s*auto\s*;/)
  })

  it('让正式 Logo 直接显示在当前主题的侧栏表面', () => {
    const brandMarkRule = appStyles.match(/\.brand-mark\s*\{([^}]*)\}/)?.[1]

    expect(brandMarkRule).toBeDefined()
    expect(brandMarkRule).toMatch(/background:\s*transparent\s*;/)
    expect(brandMarkRule).not.toMatch(/background:\s*var\(--primary\)\s*;/)
  })
})
