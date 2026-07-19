import type { TFunction } from 'i18next'
import { z } from 'zod'
import { i18n } from './i18n'
import { dataTypeOptions, platformOptions } from './workbench-data'

export const collectionDataTypeOptions = [
  {
    value: 'keyword_search',
    labelKey: 'options.dataType.keyword_search.label',
    descriptionKey: 'options.dataType.keyword_search.description',
  },
  {
    value: 'item_detail',
    labelKey: 'options.dataType.item_detail.label',
    descriptionKey: 'options.dataType.item_detail.description',
  },
  {
    value: 'account_profile',
    labelKey: 'options.dataType.account_profile.label',
    descriptionKey: 'options.dataType.account_profile.description',
  },
  {
    value: 'account_posts',
    labelKey: 'options.dataType.account_posts.label',
    descriptionKey: 'options.dataType.account_posts.description',
  },
  {
    value: 'comments',
    labelKey: 'options.dataType.comments.label',
    descriptionKey: 'options.dataType.comments.description',
  },
] as const

export type CollectionDataType = (typeof collectionDataTypeOptions)[number]['value']
export type CollectionTranslator = TFunction<'collection'>

export const accountSourceKeys = [
  'user_search',
  'content_search_authors',
  'direct_account',
  'item_author',
  'comment_authors',
  'followers',
  'followings',
  'similar_accounts',
] as const

export type AccountSourceKey = (typeof accountSourceKeys)[number]

export function getCollectionDataTypeOptions(t: CollectionTranslator) {
  return collectionDataTypeOptions.map(({ value, labelKey, descriptionKey }) => ({
    value,
    label: t(labelKey),
    description: t(descriptionKey),
  }))
}

export const AGE_RANGE_LIMITS = { min: 0, max: 130 } as const

const PLATFORM_TIME_RANGE_VALUES: Record<
  (typeof platformOptions)[number],
  readonly string[]
> = {
  TikTok: ['1', '7', '30', '180'],
  抖音: ['1', '7', '180'],
  小红书: ['1', '7', '180'],
}

export const genderFilterOptions = [
  { value: 'female', labelKey: 'options.gender.female' },
  { value: 'male', labelKey: 'options.gender.male' },
  { value: 'other', labelKey: 'options.gender.other' },
] as const

export function getGenderFilterOptions(t: CollectionTranslator) {
  return genderFilterOptions.map(({ value, labelKey }) => ({
    value,
    label: t(labelKey),
  }))
}

const ISO_COUNTRY_REGION_CODES = `
AD AE AF AG AI AL AM AO AQ AR AS AT AU AW AX AZ
BA BB BD BE BF BG BH BI BJ BL BM BN BO BQ BR BS BT BV BW BY BZ
CA CC CD CF CG CH CI CK CL CM CN CO CR CU CV CW CX CY CZ
DE DJ DK DM DO DZ EC EE EG EH ER ES ET FI FJ FK FM FO FR
GA GB GD GE GF GG GH GI GL GM GN GP GQ GR GS GT GU GW GY
HK HM HN HR HT HU ID IE IL IM IN IO IQ IR IS IT JE JM JO JP
KE KG KH KI KM KN KP KR KW KY KZ LA LB LC LI LK LR LS LT LU LV LY
MA MC MD ME MF MG MH MK ML MM MN MO MP MQ MR MS MT MU MV MW MX MY MZ
NA NC NE NF NG NI NL NO NP NR NU NZ OM PA PE PF PG PH PK PL PM PN
PR PS PT PW PY QA RE RO RS RU RW SA SB SC SD SE SG SH SI SJ SK SL
SM SN SO SR SS ST SV SX SY SZ TC TD TF TG TH TJ TK TL TM TN TO TR
TT TV TW TZ UA UG UM US UY UZ VA VC VE VG VI VN VU WF WS YE YT
ZA ZM ZW
`
  .trim()
  .split(/\s+/)

const COMMON_REGION_CODES = ['CN', 'US', 'GB', 'JP', 'KR', 'SG']
const regionNamesZh = new Intl.DisplayNames(['zh-CN'], { type: 'region' })
const regionNamesEn = new Intl.DisplayNames(['en'], { type: 'region' })

export const countryRegionOptions = [
  ...COMMON_REGION_CODES,
  ...ISO_COUNTRY_REGION_CODES.filter((code) => !COMMON_REGION_CODES.includes(code)),
].map((code) => {
  const nameZh = regionNamesZh.of(code) ?? code
  const nameEn = regionNamesEn.of(code) ?? code
  return {
    code,
    nameZh,
    nameEn,
    label: `${nameZh}（${code}）`,
  }
})

const knownRegionCodes = new Set(countryRegionOptions.map(({ code }) => code))

export function createCollectionFormSchema(t: CollectionTranslator) {
  const optionalAge = z.preprocess(
    (value) => (value === '' || Number.isNaN(value) ? undefined : value),
    z.coerce.number()
      .int(t('validation.ageInteger'))
      .min(AGE_RANGE_LIMITS.min, t('validation.ageMin', { min: AGE_RANGE_LIMITS.min }))
      .max(AGE_RANGE_LIMITS.max, t('validation.ageMax', { max: AGE_RANGE_LIMITS.max }))
      .optional(),
  )
  const collectionDataTypeSchema = z.enum(
    collectionDataTypeOptions.map(({ value }) => value),
    { error: t('validation.dataTypeValueInvalid') },
  )
  const genderSchema = z.enum(
    genderFilterOptions.map(({ value }) => value),
    { error: t('validation.genderValueInvalid') },
  )

  return z
    .object({
      platform: z.enum(platformOptions, { error: t('validation.platformRequired') }),
      accountSource: z.enum(accountSourceKeys, {
        error: t('validation.accountSourceRequired', { defaultValue: '请选择账号来源' }),
      }),
      selectedFields: z.array(z.string()).default([]),
      dataType: z.enum(dataTypeOptions, { error: t('validation.dataTypeRequired') }),
      dataTypes: z.array(collectionDataTypeSchema).min(1, t('validation.dataTypesRequired')),
      regionCode: z
        .string()
        .transform((value) => value.trim().toUpperCase())
        .refine((value) => !value || knownRegionCodes.has(value), t('validation.regionInvalid')),
      keyword: z.string().min(2, t('validation.keywordMin')).max(80, t('validation.keywordMax')),
      range: z.string().trim(),
      maxRecords: z.coerce.number()
        .min(10, t('validation.maxRecordsMin'))
        .max(5000, t('validation.maxRecordsMax')),
      budget: z.coerce.number()
        .min(0.1, t('validation.budgetMin'))
        .max(500, t('validation.budgetMax')),
      ageRangeEnabled: z.boolean(),
      ageMin: optionalAge,
      ageMax: optionalAge,
      genderFilterEnabled: z.boolean(),
      genders: z.array(genderSchema).default([]),
    })
    .superRefine((values, context) => {
      if (!values.range) {
        context.addIssue({
          code: 'custom',
          path: ['range'],
          message: t('validation.rangeRequired'),
        })
      } else if (!PLATFORM_TIME_RANGE_VALUES[values.platform].includes(values.range)) {
        context.addIssue({
          code: 'custom',
          path: ['range'],
          message: t('validation.rangeInvalid'),
        })
      }
      if (values.ageRangeEnabled) {
        if (values.ageMin === undefined) {
          context.addIssue({ code: 'custom', path: ['ageMin'], message: t('validation.ageMinRequired') })
        }
        if (values.ageMax === undefined) {
          context.addIssue({ code: 'custom', path: ['ageMax'], message: t('validation.ageMaxRequired') })
        }
        if (values.ageMin !== undefined && values.ageMax !== undefined && values.ageMin > values.ageMax) {
          context.addIssue({ code: 'custom', path: ['ageMax'], message: t('validation.ageOrder') })
        }
      }
      if (values.genderFilterEnabled && values.genders.length === 0) {
        context.addIssue({ code: 'custom', path: ['genders'], message: t('validation.gendersRequired') })
      }
    })
}

export const collectionFormSchema = createCollectionFormSchema(
  i18n.getFixedT('zh-CN', 'collection'),
)

const REGION_CAPABLE_TYPES: Record<(typeof platformOptions)[number], ReadonlySet<CollectionDataType>> = {
  TikTok: new Set(['keyword_search', 'account_posts', 'comments']),
  抖音: new Set(['keyword_search', 'comments']),
  小红书: new Set(['keyword_search', 'comments']),
}

export function supportsRegionSelection(
  platform: (typeof platformOptions)[number],
  dataTypes: readonly CollectionDataType[],
) {
  return dataTypes.some((dataType) => REGION_CAPABLE_TYPES[platform].has(dataType))
}
