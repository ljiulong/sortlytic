import { createElement } from 'react'
import { renderToStaticMarkup } from 'react-dom/server'
import { describe, expect, it, vi } from 'vitest'
import TikhubSettingsPanel from './TikhubSettingsPanel'

describe('TikhubSettingsPanel', () => {
  it('同时显示充值余额、免费额度、合计与每日用量', () => {
    const markup = renderToStaticMarkup(
      createElement(TikhubSettingsPanel, {
        isBusy: false,
        onSaveAndTest: vi.fn(),
        result: {
          success: true,
          base_url: 'https://api.tikhub.io',
          balance: 1.25,
          free_credit: 0.05,
          available_credit: 1.3,
          daily_usage_json: { total_requests: 12 },
          message: 'TikHub Token 可用',
        },
      }),
    )

    expect(markup).toContain('充值余额')
    expect(markup).toContain('$1.25')
    expect(markup).toContain('免费额度')
    expect(markup).toContain('$0.05')
    expect(markup).toContain('可用额度合计')
    expect(markup).toContain('$1.30')
    expect(markup).toContain('今日用量')
    expect(markup).toContain('12 次请求')
  })
})
