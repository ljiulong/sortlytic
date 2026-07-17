import type { AppSelectOption } from './AppSelect'
import { countryRegionOptions } from './collection-options'
import { platformOptions } from './workbench-data'

export const platformSelectOptions = platformOptions.map((platform) => ({
  value: platform,
  label: platform,
})) satisfies AppSelectOption[]

export const countryRegionSelectOptions = countryRegionOptions.map(({
  code,
  nameZh,
  nameEn,
}) => ({
  value: code,
  label: nameZh,
  description: nameEn,
  meta: code,
  keywords: `${nameZh} ${nameEn} ${code}`,
})) satisfies AppSelectOption[]
