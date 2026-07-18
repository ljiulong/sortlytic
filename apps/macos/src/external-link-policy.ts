export const allowedExternalUrls = [
  'https://github.com/ljiulong/sortlytic',
  'https://user.tikhub.io/register',
  'https://user.tikhub.io/login',
  'https://docs.tikhub.io/',
  'https://tikhub.io/getting-started',
  'https://tikhub.io/pricing',
] as const

export type AllowedExternalUrl = typeof allowedExternalUrls[number]

export function isAllowedExternalUrl(url: string): url is AllowedExternalUrl {
  return allowedExternalUrls.some((allowedUrl) => allowedUrl === url)
}

export async function openAllowedExternalUrl(url: string) {
  if (!isAllowedExternalUrl(url)) throw new Error('不允许打开未授权的外部链接')
  const { openUrl } = await import('@tauri-apps/plugin-opener')
  await openUrl(url)
}
