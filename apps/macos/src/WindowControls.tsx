import { getCurrentWindow } from '@tauri-apps/api/window'
import { useTranslation } from 'react-i18next'
import './i18n'
import './WindowControls.css'

function WindowControls() {
  const { t } = useTranslation('common')
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return null

  return (
    <div className="window-chrome" data-tauri-drag-region>
      <div className="window-controls">
        <button
          aria-label={t('window.close')}
          title={t('window.close')}
          className="window-control window-control-close"
          type="button"
          onClick={(event) => {
            event.stopPropagation()
            void getCurrentWindow().close()
          }}
        />
        <button
          aria-label={t('window.minimize')}
          title={t('window.minimize')}
          className="window-control window-control-minimize"
          type="button"
          onClick={(event) => {
            event.stopPropagation()
            void getCurrentWindow().minimize()
          }}
        />
        <button
          aria-label={t('window.toggleSize')}
          title={t('window.toggleSize')}
          className="window-control window-control-maximize"
          type="button"
          onClick={(event) => {
            event.stopPropagation()
            void getCurrentWindow().toggleMaximize()
          }}
        />
      </div>
    </div>
  )
}

export default WindowControls
