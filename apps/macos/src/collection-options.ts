export const collectionDataTypeOptions = [
  {
    value: 'keyword_search',
    label: '搜索结果账号',
    description: '从关键词搜索结果提取公开作者账号。',
  },
  {
    value: 'item_detail',
    label: '作品/笔记作者',
    description: '读取作品或笔记详情中的公开作者信息。',
  },
  {
    value: 'account_profile',
    label: '账号公开信息',
    description: '补全账号简介、粉丝数和公开地区等资料。',
  },
  {
    value: 'account_posts',
    label: '账号作品所属账号',
    description: '读取账号作品列表并更新作品数和最近发文时间。',
  },
  {
    value: 'comments',
    label: '评论用户',
    description: '从公开评论中提取评论用户账号。',
  },
] as const

export type CollectionDataType = (typeof collectionDataTypeOptions)[number]['value']

export const AGE_RANGE_LIMITS = { min: 0, max: 130 } as const

export const genderFilterOptions = [
  { value: 'female', label: '女性' },
  { value: 'male', label: '男性' },
  { value: 'other', label: '其他明确性别' },
] as const

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
const regionNames = new Intl.DisplayNames(['zh-CN'], { type: 'region' })

export const countryRegionOptions = [
  ...COMMON_REGION_CODES,
  ...ISO_COUNTRY_REGION_CODES.filter((code) => !COMMON_REGION_CODES.includes(code)),
].map((code) => ({
  code,
  label: `${regionNames.of(code) ?? code}（${code}）`,
}))

const optionalAge = z.preprocess(
  (value) => (value === '' || Number.isNaN(value) ? undefined : value),
  z.coerce.number().int('年龄必须是整数').min(AGE_RANGE_LIMITS.min).max(AGE_RANGE_LIMITS.max).optional(),
)
const knownRegionCodes = new Set(countryRegionOptions.map(({ code }) => code))

export const collectionFormSchema = z
  .object({
    platform: z.enum(platformOptions),
    dataType: z.enum(dataTypeOptions),
    dataTypes: z.array(z.enum(collectionDataTypeOptions.map(({ value }) => value))).min(1, '至少选择一种目标数据'),
    regionCode: z
      .string()
      .transform((value) => value.trim().toUpperCase())
      .refine((value) => !value || knownRegionCodes.has(value), '请选择下拉表中的国家/地区'),
    keyword: z.string().min(2, '请输入关键词或账号').max(80, '关键词过长'),
    range: z.string().min(4, '请选择时间范围'),
    maxRecords: z.coerce.number().min(10, '至少 10 条').max(5000, 'MVP 单任务上限为 5000 条'),
    budget: z.coerce.number().min(1, '请输入成本上限').max(500, 'MVP 单任务上限为 500'),
    ageRangeEnabled: z.boolean(),
    ageMin: optionalAge,
    ageMax: optionalAge,
    genderFilterEnabled: z.boolean(),
    genders: z.array(z.enum(genderFilterOptions.map(({ value }) => value))).default([]),
  })
  .superRefine((values, context) => {
    if (values.ageRangeEnabled) {
      if (values.ageMin === undefined) {
        context.addIssue({ code: 'custom', path: ['ageMin'], message: '请输入最小年龄' })
      }
      if (values.ageMax === undefined) {
        context.addIssue({ code: 'custom', path: ['ageMax'], message: '请输入最大年龄' })
      }
      if (values.ageMin !== undefined && values.ageMax !== undefined && values.ageMin > values.ageMax) {
        context.addIssue({ code: 'custom', path: ['ageMax'], message: '最大年龄不能小于最小年龄' })
      }
    }
    if (values.genderFilterEnabled && values.genders.length === 0) {
      context.addIssue({ code: 'custom', path: ['genders'], message: '请至少选择一种明确性别' })
    }
  })

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
import { z } from 'zod'
import { dataTypeOptions, platformOptions } from './workbench-data'
