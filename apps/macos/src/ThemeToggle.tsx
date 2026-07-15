import { Moon, Sun } from 'lucide-react'
import { useEffect, useState } from 'react'

type Theme = 'light' | 'dark'

const themeStorageKey = 'sortlytic-theme'

function readStoredTheme(): Theme | null {
  if (typeof window === 'undefined') return null
  try {
    const storedTheme = window.localStorage.getItem(themeStorageKey)
    return storedTheme === 'light' || storedTheme === 'dark' ? storedTheme : null
  } catch {
    return null
  }
}

function readSystemTheme(): Theme {
  if (typeof window === 'undefined') return 'light'
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'
}

function ThemeToggle() {
  const [theme, setTheme] = useState<Theme>(() => readStoredTheme() ?? readSystemTheme())

  useEffect(() => {
    document.documentElement.dataset.theme = theme
    document.documentElement.style.colorScheme = theme
  }, [theme])

  useEffect(() => {
    if (readStoredTheme()) return

    const mediaQuery = window.matchMedia('(prefers-color-scheme: dark)')
    const syncSystemTheme = (event: MediaQueryListEvent) => {
      if (!readStoredTheme()) setTheme(event.matches ? 'dark' : 'light')
    }

    mediaQuery.addEventListener('change', syncSystemTheme)
    return () => mediaQuery.removeEventListener('change', syncSystemTheme)
  }, [])

  const nextTheme = theme === 'dark' ? 'light' : 'dark'
  const label = `切换为${nextTheme === 'dark' ? '深色' : '浅色'}主题`
  const Icon = theme === 'dark' ? Sun : Moon

  return (
    <button
      aria-label={label}
      aria-pressed={theme === 'dark'}
      className="toolbar-icon-button theme-toggle"
      title={label}
      type="button"
      onClick={() => {
        setTheme(nextTheme)
        try {
          window.localStorage.setItem(themeStorageKey, nextTheme)
        } catch {
          // 存储不可用时仍允许本次会话切换主题。
        }
      }}
    >
      <Icon size={17} aria-hidden="true" />
    </button>
  )
}

export default ThemeToggle
