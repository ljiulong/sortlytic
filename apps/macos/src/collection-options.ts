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
