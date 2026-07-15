import WindowControls from './WindowControls'

function AppLogo() {
  return (
    <>
      <WindowControls />
      <svg aria-hidden="true" fill="none" height="22" viewBox="0 0 32 32" width="22">
        <path d="m6 10 10-4.5L26 10l-10 4.5L6 10Z" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" strokeWidth="2.1" />
        <path d="m6 15.5 10 4.5 10-4.5M6 21l10 4.5L26 21" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" strokeWidth="2.1" />
      </svg>
    </>
  )
}

export default AppLogo
