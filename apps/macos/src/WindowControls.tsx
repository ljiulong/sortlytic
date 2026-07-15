import { getCurrentWindow } from '@tauri-apps/api/window'
import './WindowControls.css'

function WindowControls() {
  if (typeof window === 'undefined' || !('__TAURI_INTERNALS__' in window)) return null

  return (
    <div className="window-chrome" data-tauri-drag-region>
      <div className="window-controls">
        <button
          aria-label="关闭窗口"
          className="window-control window-control-close"
          type="button"
          onClick={(event) => {
            event.stopPropagation()
            void getCurrentWindow().close()
          }}
        />
        <button
          aria-label="最小化窗口"
          className="window-control window-control-minimize"
          type="button"
          onClick={(event) => {
            event.stopPropagation()
            void getCurrentWindow().minimize()
          }}
        />
        <button
          aria-label="切换窗口大小"
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
