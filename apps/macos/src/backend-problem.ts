export type BackendProblem = {
  code: string
  stage: string
  message: string
  retryable: boolean
  safeDetails: Record<string, unknown>
}

const sensitiveKeyPattern = /(?:api[_-]?key|authorization|credential|password|secret|token)/i
const sensitiveQueryKeyPattern = /^(?:api[_-]?key|access[_-]?token|authorization|password|secret|token)$/i

export function normalizeBackendProblem(error: unknown): BackendProblem {
  if (typeof error === 'string' && error.trim()) {
    return problemFromParts(undefined, undefined, error, undefined, undefined)
  }
  if (error && typeof error === 'object') {
    const source = error as Record<string, unknown>
    const code = nonEmptyString(source.code)
    const stage = nonEmptyString(source.stage)
    const message = nonEmptyString(source.message)
    const safeDetails = source.safe_details ?? source.safeDetails
    if (code || stage || message) {
      return problemFromParts(code, stage, message, source.retryable, safeDetails)
    }
  }
  return {
    code: 'UNKNOWN_ERROR',
    stage: 'unknown',
    message: '未能读取完整错误详情',
    retryable: false,
    safeDetails: {},
  }
}

function problemFromParts(
  code: string | undefined,
  stage: string | undefined,
  message: string | undefined,
  retryable: unknown,
  safeDetails: unknown,
): BackendProblem {
  return {
    code: code ?? 'UNCLASSIFIED_ERROR',
    stage: stage ?? 'unknown',
    message: sanitizeText(
      message
      ?? (code ? `后端返回错误 ${code}` : stage ? `后端在 ${stage} 阶段失败` : '后端调用失败'),
    ),
    retryable: retryable === true,
    safeDetails: sanitizeSafeDetails(safeDetails),
  }
}

function sanitizeSafeDetails(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) return {}
  const sanitized: Record<string, unknown> = {}
  for (const [key, detail] of Object.entries(value)) {
    if (sensitiveKeyPattern.test(key)) continue
    const safeValue = sanitizeSafeValue(detail)
    if (safeValue !== undefined) sanitized[key] = safeValue
  }
  return sanitized
}

function sanitizeSafeValue(value: unknown): unknown {
  if (typeof value === 'string') return sanitizeText(value)
  if (typeof value === 'number' || typeof value === 'boolean' || value === null) return value
  if (Array.isArray(value)) {
    return value
      .map(sanitizeSafeValue)
      .filter((item) => item !== undefined)
  }
  if (value && typeof value === 'object') return sanitizeSafeDetails(value)
  return undefined
}

function sanitizeText(value: string) {
  return value
    .replace(/authorization\s*:\s*(?:bearer\s+)?[^\s，。;]+/gi, 'Authorization: [已隐藏]')
    .replace(/\bsk-[a-z0-9_-]+\b/gi, '[已隐藏]')
    .replace(/([?&](?:api[_-]?key|access[_-]?token|password|secret|token)=)[^&\s]+/gi, '$1已隐藏')
    .replace(/https?:\/\/[^\s，。]+/g, sanitizeUrl)
}

function sanitizeUrl(rawUrl: string) {
  try {
    const url = new URL(rawUrl)
    for (const key of [...url.searchParams.keys()]) {
      if (sensitiveQueryKeyPattern.test(key)) url.searchParams.set(key, '已隐藏')
    }
    return url.toString()
  } catch {
    return rawUrl
  }
}

function nonEmptyString(value: unknown) {
  return typeof value === 'string' && value.trim() ? value.trim() : undefined
}
