import WindowControls from './WindowControls'

const logoUrl = new URL('../src-tauri/icons/icon.png', import.meta.url).href

function AppLogo() {
  return (
    <>
      <WindowControls />
      <img alt="" aria-hidden="true" height="32" src={logoUrl} width="32" />
    </>
  )
}

export default AppLogo
