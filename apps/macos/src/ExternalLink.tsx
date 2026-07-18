import type { AnchorHTMLAttributes, MouseEvent, ReactNode } from 'react'
import {
  openAllowedExternalUrl,
  type AllowedExternalUrl,
} from './external-link-policy'

export type { AllowedExternalUrl } from './external-link-policy'

export type ExternalLinkProps = Omit<AnchorHTMLAttributes<HTMLAnchorElement>, 'href'> & {
  href: AllowedExternalUrl
  children: ReactNode
  ariaLabel?: string
  onOpenError: (error: unknown) => void
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
