import type { CollectionFormInput } from './CollectionBuilder'

export const newCollectionFormDefaults = {
  platform: undefined,
  dataType: undefined,
  dataTypes: [],
  regionCode: '',
  keyword: '',
  range: '',
  maxRecords: undefined,
  budget: undefined,
  ageRangeEnabled: false,
  genderFilterEnabled: false,
  genders: [],
} satisfies Partial<CollectionFormInput>
