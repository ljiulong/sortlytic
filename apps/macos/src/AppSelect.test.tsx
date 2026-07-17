import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'
import AppSelect from './AppSelect'

const options = [
  { value: 'CN', label: '中国大陆', meta: 'CN' },
  { value: 'US', label: '美国', meta: 'US' },
  { value: 'JP', label: '日本', meta: 'JP' },
]

describe('AppSelect', () => {
  it('关闭态使用应用内列表框触发器而不是原生下拉菜单', () => {
    const markup = renderToStaticMarkup(createElement(AppSelect, {
      id: 'region-code',
      onChange: vi.fn(),
      options,
      placeholder: '请选择国家/地区',
      searchable: true,
      value: 'US',
    }))

    expect(markup).toContain('id="region-code"')
    expect(markup).toContain('aria-haspopup="listbox"')
    expect(markup).toContain('aria-expanded="false"')
    expect(markup).toContain('美国')
    expect(markup).toContain('US')
    expect(markup).not.toContain('<select')
  })

})
