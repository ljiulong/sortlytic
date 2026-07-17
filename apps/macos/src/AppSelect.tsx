import { Check, ChevronDown, Search } from 'lucide-react'
import { useTranslation } from 'react-i18next'
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
} from 'react'
import './AppSelect.css'
import './i18n'

export type AppSelectOption = {
  value: string
  label: string
  meta?: string
  description?: string
  keywords?: string
}

type AppSelectProps = {
  id: string
  value: string
  options: readonly AppSelectOption[]
  placeholder: string
  onChange: (value: string) => void
  ariaDescribedBy?: string
  disabled?: boolean
  invalid?: boolean
  searchable?: boolean
  searchPlaceholder?: string
  emptyLabel?: string
}

function nextOptionIndex(current: number, length: number, direction: 1 | -1) {
  if (length === 0) return -1
  if (current < 0) return direction === 1 ? 0 : length - 1
  return (current + direction + length) % length
}

function normalizeSearch(value: string) {
  return value.trim().toLocaleLowerCase()
}

function optionId(selectId: string, value: string) {
  return `${selectId}-option-${value.replace(/[^a-zA-Z0-9_-]/g, '-')}`
}

function AppSelect({
  id,
  value,
  options,
  placeholder,
  onChange,
  ariaDescribedBy,
  disabled = false,
  invalid = false,
  searchable = false,
  searchPlaceholder,
  emptyLabel,
}: AppSelectProps) {
  const { t } = useTranslation('common')
  const resolvedSearchPlaceholder = searchPlaceholder ?? t('select.searchOptions')
  const resolvedEmptyLabel = emptyLabel ?? t('select.noMatchingOptions')
  const [isOpen, setIsOpen] = useState(false)
  const [query, setQuery] = useState('')
  const [activeIndex, setActiveIndex] = useState(-1)
  const rootRef = useRef<HTMLDivElement>(null)
  const triggerRef = useRef<HTMLButtonElement>(null)
  const searchRef = useRef<HTMLInputElement>(null)
  const listboxId = `${id}-listbox`
  const selectedOption = options.find((option) => option.value === value)
  const normalizedQuery = normalizeSearch(query)
  const filteredOptions = useMemo(() => {
    if (!normalizedQuery) return options
    return options.filter((option) => normalizeSearch([
      option.label,
      option.value,
      option.meta,
      option.keywords,
    ].filter(Boolean).join(' ')).includes(normalizedQuery))
  }, [normalizedQuery, options])
  const activeOption = activeIndex >= 0 ? filteredOptions[activeIndex] : undefined
  const activeOptionId = isOpen && activeOption
    ? optionId(id, activeOption.value)
    : undefined

  useEffect(() => {
    if (!isOpen) return
    const selectedIndex = filteredOptions.findIndex((option) => option.value === value)
    setActiveIndex(selectedIndex >= 0 ? selectedIndex : filteredOptions.length > 0 ? 0 : -1)
  }, [filteredOptions, isOpen, value])

  useEffect(() => {
    if (!isOpen || !searchable) return
    const frame = requestAnimationFrame(() => searchRef.current?.focus())
    return () => cancelAnimationFrame(frame)
  }, [isOpen, searchable])

  useEffect(() => {
    const closeFromOutside = (event: PointerEvent) => {
      if (!rootRef.current?.contains(event.target as Node)) setIsOpen(false)
    }
    document.addEventListener('pointerdown', closeFromOutside)
    return () => document.removeEventListener('pointerdown', closeFromOutside)
  }, [])

  useEffect(() => {
    if (disabled) setIsOpen(false)
  }, [disabled])

  const open = () => {
    if (disabled) return
    setQuery('')
    const selectedIndex = options.findIndex((option) => option.value === value)
    setActiveIndex(selectedIndex >= 0 ? selectedIndex : options.length > 0 ? 0 : -1)
    setIsOpen(true)
  }

  const close = (restoreFocus = false) => {
    setIsOpen(false)
    setQuery('')
    if (restoreFocus) requestAnimationFrame(() => triggerRef.current?.focus())
  }

  const choose = (option: AppSelectOption) => {
    onChange(option.value)
    close(true)
  }

  const move = (direction: 1 | -1) => {
    setActiveIndex((current) => nextOptionIndex(current, filteredOptions.length, direction))
  }

  const moveToBoundary = (boundary: 'first' | 'last') => {
    if (filteredOptions.length === 0) return
    setActiveIndex(boundary === 'first' ? 0 : filteredOptions.length - 1)
  }

  const chooseActiveOption = () => {
    const option = filteredOptions[activeIndex]
    if (option) choose(option)
  }

  const handleTriggerKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      if (!isOpen) open()
      else move(event.key === 'ArrowDown' ? 1 : -1)
      return
    }
    if (event.key === 'Enter' || event.key === ' ') {
      event.preventDefault()
      if (isOpen) chooseActiveOption()
      else open()
      return
    }
    if (event.key === 'Home' && isOpen) {
      event.preventDefault()
      moveToBoundary('first')
      return
    }
    if (event.key === 'End' && isOpen) {
      event.preventDefault()
      moveToBoundary('last')
      return
    }
    if (event.key === 'Escape' && isOpen) {
      event.preventDefault()
      close(true)
    }
  }

  const handleSearchKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault()
      move(event.key === 'ArrowDown' ? 1 : -1)
      return
    }
    if (event.key === 'Enter') {
      event.preventDefault()
      chooseActiveOption()
      return
    }
    if (event.key === 'Home') {
      event.preventDefault()
      moveToBoundary('first')
      return
    }
    if (event.key === 'End') {
      event.preventDefault()
      moveToBoundary('last')
      return
    }
    if (event.key === 'Escape') {
      event.preventDefault()
      close(true)
    }
  }

  return (
    <div className="app-select" data-open={isOpen} ref={rootRef}>
      <button
        id={id}
        ref={triggerRef}
        className="app-select__trigger"
        type="button"
        aria-controls={isOpen ? listboxId : undefined}
        aria-describedby={ariaDescribedBy}
        aria-expanded={isOpen}
        aria-haspopup="listbox"
        aria-invalid={invalid}
        aria-activedescendant={!searchable ? activeOptionId : undefined}
        disabled={disabled}
        role={!searchable ? 'combobox' : undefined}
        onClick={() => isOpen ? close() : open()}
        onKeyDown={handleTriggerKeyDown}
      >
        <span className="app-select__value" data-placeholder={!selectedOption}>
          <span>{selectedOption?.label ?? placeholder}</span>
          {selectedOption?.meta ? <small>{selectedOption.meta}</small> : null}
        </span>
        <ChevronDown className="app-select__chevron" size={15} aria-hidden="true" />
      </button>

      {isOpen ? (
        <div className="app-select__popover">
          {searchable ? (
            <label className="app-select__search">
              <Search size={14} aria-hidden="true" />
              <input
                ref={searchRef}
                role="combobox"
                aria-label={resolvedSearchPlaceholder}
                aria-activedescendant={activeOptionId}
                aria-autocomplete="list"
                aria-controls={listboxId}
                aria-expanded={isOpen}
                aria-haspopup="listbox"
                autoComplete="off"
                placeholder={resolvedSearchPlaceholder}
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                onKeyDown={handleSearchKeyDown}
              />
            </label>
          ) : null}
          <div
            id={listboxId}
            className="app-select__listbox"
            role="listbox"
            aria-labelledby={id}
          >
            {filteredOptions.map((option, index) => {
              const selected = option.value === value
              return (
                <button
                  id={optionId(id, option.value)}
                  className="app-select__option"
                  type="button"
                  role="option"
                  aria-selected={selected}
                  tabIndex={-1}
                  data-active={index === activeIndex}
                  key={option.value}
                  onClick={() => choose(option)}
                  onMouseEnter={() => setActiveIndex(index)}
                >
                  <span>
                    <strong>{option.label}</strong>
                    {option.description ? <small>{option.description}</small> : null}
                  </span>
                  {option.meta ? <code>{option.meta}</code> : null}
                  <Check className="app-select__check" size={14} aria-hidden="true" />
                </button>
              )
            })}
            {filteredOptions.length === 0 ? (
              <p className="app-select__empty">{resolvedEmptyLabel}</p>
            ) : null}
          </div>
        </div>
      ) : null}
    </div>
  )
}

export default AppSelect
