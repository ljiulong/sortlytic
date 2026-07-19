import { useMemo } from 'react'
import type { FieldErrors, UseFormRegister } from 'react-hook-form'
import { useTranslation } from 'react-i18next'
import type { CollectionFormInput } from './CollectionBuilder'
import {
  AGE_RANGE_LIMITS,
  getGenderFilterOptions,
} from './collection-options'
import { i18n } from './i18n'

type CollectionFilterFieldsProps = {
  ageFilterSupported: boolean
  ageRangeEnabled: boolean
  capabilityReady: boolean
  errors: FieldErrors<CollectionFormInput>
  genderFilterEnabled: boolean
  genderFilterSupported: boolean
  platformSelected: boolean
  register: UseFormRegister<CollectionFormInput>
}

function CollectionFilterFields({
  ageFilterSupported,
  ageRangeEnabled,
  capabilityReady,
  errors,
  genderFilterEnabled,
  genderFilterSupported,
  platformSelected,
  register,
}: CollectionFilterFieldsProps) {
  const { t } = useTranslation('collection', { i18n })
  const genderOptions = useMemo(() => getGenderFilterOptions(t), [t])
  const descriptionKey = (
    supported: boolean,
    supportedKey: string,
    unsupportedKey: string,
  ) => !platformSelected
    ? 'fields.filterRequiresPlatform'
    : !capabilityReady
      ? 'fields.filterCapabilityLoading'
      : supported
        ? supportedKey
        : unsupportedKey

  return (
    <div className="collection-builder__filter-grid">
      <fieldset
        aria-disabled={!ageFilterSupported}
        className="collection-builder__filter-block"
        data-enabled={ageRangeEnabled}
        data-supported={ageFilterSupported}
      >
        <legend className="collection-builder__visually-hidden">{t('fields.ageRange')}</legend>
        <label className="collection-builder__filter-toggle">
          <input
            data-ui="sortlytic-checkbox"
            disabled={!ageFilterSupported}
            type="checkbox"
            {...register('ageRangeEnabled')}
          />
          <span>
            <strong>{t('fields.ageRange')}</strong>
            <small>{t(descriptionKey(
              ageFilterSupported,
              'fields.ageRangeDescription',
              'fields.ageRangeUnavailable',
            ))}</small>
          </span>
        </label>
        {ageRangeEnabled && ageFilterSupported ? (
          <div className="collection-builder__age-inputs">
            <label>
              <span>{t('fields.minimumAge')}</span>
              <input
                aria-describedby={errors.ageMin ? 'age-min-error' : undefined}
                aria-invalid={Boolean(errors.ageMin)}
                min={AGE_RANGE_LIMITS.min}
                max={AGE_RANGE_LIMITS.max}
                placeholder={t('placeholders.minimumAge')}
                type="number"
                {...register('ageMin', { valueAsNumber: true })}
              />
            </label>
            <span aria-hidden="true">{t('fields.ageRangeSeparator')}</span>
            <label>
              <span>{t('fields.maximumAge')}</span>
              <input
                aria-describedby={errors.ageMax ? 'age-max-error' : undefined}
                aria-invalid={Boolean(errors.ageMax)}
                min={AGE_RANGE_LIMITS.min}
                max={AGE_RANGE_LIMITS.max}
                placeholder={t('placeholders.maximumAge')}
                type="number"
                {...register('ageMax', { valueAsNumber: true })}
              />
            </label>
          </div>
        ) : null}
        <FilterError id="age-min-error" message={errors.ageMin?.message} />
        <FilterError id="age-max-error" message={errors.ageMax?.message} />
      </fieldset>

      <fieldset
        aria-disabled={!genderFilterSupported}
        className="collection-builder__filter-block"
        data-enabled={genderFilterEnabled}
        data-supported={genderFilterSupported}
      >
        <legend className="collection-builder__visually-hidden">{t('fields.gender')}</legend>
        <label className="collection-builder__filter-toggle">
          <input
            data-ui="sortlytic-checkbox"
            disabled={!genderFilterSupported}
            type="checkbox"
            {...register('genderFilterEnabled')}
          />
          <span>
            <strong>{t('fields.gender')}</strong>
            <small>{t(descriptionKey(
              genderFilterSupported,
              'fields.genderDescription',
              'fields.genderUnavailable',
            ))}</small>
          </span>
        </label>
        {genderFilterEnabled && genderFilterSupported ? (
          <div className="collection-builder__gender-options">
            {genderOptions.map((item) => (
              <label key={item.value}>
                <input
                  data-ui="sortlytic-checkbox"
                  type="checkbox"
                  value={item.value}
                  {...register('genders')}
                />
                <span>{item.label}</span>
              </label>
            ))}
          </div>
        ) : null}
        <FilterError id="genders-error" message={errors.genders?.message} />
      </fieldset>
    </div>
  )
}

function FilterError({ id, message }: { id: string; message?: string }) {
  return message ? <small className="form-error" id={id}>{message}</small> : null
}

export default CollectionFilterFields
