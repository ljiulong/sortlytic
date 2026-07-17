export type UpdatePreferences = {
  autoCheck: boolean
  autoDownload: boolean
}

export const updatePreferencesStorageKey = 'sortlytic-update-preferences-v1'

export const defaultUpdatePreferences: Readonly<UpdatePreferences> = {
  autoCheck: true,
  autoDownload: false,
}

function createDefaultPreferences(): UpdatePreferences {
  return { ...defaultUpdatePreferences }
}

function isUpdatePreferences(value: unknown): value is UpdatePreferences {
  if (typeof value !== 'object' || value === null) return false

  const preferences = value as Record<string, unknown>
  if (
    typeof preferences.autoCheck !== 'boolean'
    || typeof preferences.autoDownload !== 'boolean'
  ) {
    return false
  }

  return preferences.autoCheck || !preferences.autoDownload
}

function normalizeUpdatePreferences(
  preferences: UpdatePreferences,
): UpdatePreferences {
  return {
    autoCheck: preferences.autoCheck,
    autoDownload: preferences.autoCheck && preferences.autoDownload,
  }
}

export function readUpdatePreferences(): UpdatePreferences {
  try {
    const storage = globalThis.localStorage
    if (!storage) return createDefaultPreferences()

    const storedValue = storage.getItem(updatePreferencesStorageKey)
    if (!storedValue) return createDefaultPreferences()

    const parsedValue: unknown = JSON.parse(storedValue)
    return isUpdatePreferences(parsedValue)
      ? { ...parsedValue }
      : createDefaultPreferences()
  } catch {
    return createDefaultPreferences()
  }
}

export function saveUpdatePreferences(preferences: UpdatePreferences): void {
  try {
    const storage = globalThis.localStorage
    if (!storage) return

    storage.setItem(
      updatePreferencesStorageKey,
      JSON.stringify(normalizeUpdatePreferences(preferences)),
    )
  } catch {
    // 偏好无法持久化时保持本次会话可用，由调用方继续使用内存状态。
  }
}
