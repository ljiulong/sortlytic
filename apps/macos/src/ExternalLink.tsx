import type { AnchorHTMLAttributes, MouseEvent, ReactNode } from 'react'

export const allowedExternalUrls = [
  'https://github.com/ljiulong/sortlytic',
  'https://user.tikhub.io/register',
  'https://user.tikhub.io/login',
  'https://docs.tikhub.io/',
  'https://tikhub.io/getting-started',
  'https://tikhub.io/pricing',
] as const

export type AllowedExternalUrl = typeof allowedExternalUrls[number]

export type ExternalLinkProps = Omit<AnchorHTMLAttributes<HTMLAnchorElement>, 'href'> & {
  href: AllowedExternalUrl
  children: ReactNode
  ariaLabel?: string
  onOpenError: (error: unknown) => void
}

export function isAllowedExternalUrl(url: string): url is AllowedExternalUrl {
  return allowedExternalUrls.some((allowedUrl) => allowedUrl === url)
}

export async function openAllowedExternalUrl(url: string) {
  if (!isAllowedExternalUrl(url)) throw new Error('不允许打开未授权的外部链接')
  const { openUrl } = await import('@tauri-apps/plugin-opener')
  await openUrl(url)
}

function isTauriRuntime() {
  return typeof window !== 'undefined'
    && Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__)
}

function ExternalLink({
  ariaLabel,
  children,
  href,
  onClick,
  onOpenError,
  ...anchorProps
}: ExternalLinkProps) {
  const handleClick = (event: MouseEvent<HTMLAnchorElement>) => {
    onClick?.(event)
    if (event.defaultPrevented || !isTauriRuntime()) return

    event.preventDefault()
    void openAllowedExternalUrl(href).catch(onOpenError)
  }

  return (
    <a
      {...anchorProps}
      aria-label={ariaLabel}
      data-external-link="true"
      href={href}
      rel="noreferrer"
      target="_blank"
      onClick={handleClick}
    >
      {children}
    </a>
  )
}

export default ExternalLink
