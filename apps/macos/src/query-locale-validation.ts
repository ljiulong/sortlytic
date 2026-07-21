const primaryLocales: Record<string, string> = {
  GB: 'en-GB', US: 'en-US', CA: 'en-CA', AU: 'en-AU', NZ: 'en-NZ', IE: 'en-IE',
  SG: 'en-SG', ZA: 'en-ZA', NG: 'en-NG', KE: 'en-KE', CN: 'zh-CN', HK: 'zh-HK',
  MO: 'zh-MO', TW: 'zh-TW', JP: 'ja-JP', KR: 'ko-KR', FR: 'fr-FR', DE: 'de-DE',
  AT: 'de-AT', CH: 'de-CH', ES: 'es-ES', MX: 'es-MX', AR: 'es-AR', CL: 'es-CL',
  CO: 'es-CO', PE: 'es-PE', IT: 'it-IT', PT: 'pt-PT', BR: 'pt-BR', NL: 'nl-NL',
  BE: 'nl-BE', SE: 'sv-SE', NO: 'no-NO', DK: 'da-DK', FI: 'fi-FI', PL: 'pl-PL',
  CZ: 'cs-CZ', GR: 'el-GR', RO: 'ro-RO', HU: 'hu-HU', UA: 'uk-UA', RU: 'ru-RU',
  TR: 'tr-TR', IL: 'he-IL', SA: 'ar-SA', AE: 'ar-AE', EG: 'ar-EG', MA: 'ar-MA',
  IN: 'hi-IN', PK: 'ur-PK', BD: 'bn-BD', TH: 'th-TH', VN: 'vi-VN', ID: 'id-ID',
  MY: 'ms-MY', PH: 'fil-PH',
}

export function validateQueryLocale(
  queryLocale: string,
  regionCode: string,
  keywordSource: boolean,
) {
  const locale = queryLocale.trim()
  if (!keywordSource) {
    return locale ? '直接账号、作品或 URL 来源不得设置目标检索语言' : ''
  }
  if (!locale) return '请填写目标检索语言，格式为 language-REGION，例如 en-GB'
  if (!/^[a-z]{2,3}-[A-Z]{2}$/.test(locale)) {
    return '目标检索语言必须使用 language-REGION 格式，例如 en-GB'
  }
  const expected = primaryLocales[regionCode]
  if (expected && locale !== expected) return `目标地区 ${regionCode} 的主检索语言必须为 ${expected}`
  if (regionCode && locale.slice(-2) !== regionCode) {
    return `目标检索语言 ${locale} 必须与国家地区 ${regionCode} 一致`
  }
  return ''
}
