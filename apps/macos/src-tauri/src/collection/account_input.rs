use super::AccountSourceInputKind;
use crate::domain::{AppError, AppErrorStage, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AccountSourceParamKey {
  Keyword,
  AccountId,
  ItemId,
  ShareText,
}

impl AccountSourceParamKey {
  pub(super) fn as_str(self) -> &'static str {
    match self {
      Self::Keyword => "keyword",
      Self::AccountId => "account_id",
      Self::ItemId => "item_id",
      Self::ShareText => "share_text",
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NormalizedAccountSourceInput {
  pub(super) key: AccountSourceParamKey,
  pub(super) value: String,
}

pub(super) fn normalize_account_source_input(
  platform: &str,
  account_source: &str,
  input_kind: AccountSourceInputKind,
  input: &str,
) -> AppResult<NormalizedAccountSourceInput> {
  let input = input.trim();
  if input.is_empty() {
    return Err(validation_error("账号来源输入不能为空"));
  }
  match input_kind {
    AccountSourceInputKind::Keyword => Ok(normalized(AccountSourceParamKey::Keyword, input)),
    AccountSourceInputKind::Item => normalize_item_input(platform, input),
    AccountSourceInputKind::Account => normalize_account_input(platform, account_source, input),
  }
}

fn normalize_item_input(platform: &str, input: &str) -> AppResult<NormalizedAccountSourceInput> {
  let Some(url) = ParsedUrl::parse(input)? else {
    return validate_raw_item_id(platform, input);
  };
  match platform {
    "tiktok" if url.host_matches("tiktok.com") => url
      .segment_after("video")
      .filter(|value| is_ascii_digits(value))
      .map(|value| normalized(AccountSourceParamKey::ItemId, value))
      .ok_or_else(|| validation_error("TikTok 作品链接必须包含可识别的数字 video ID；短链接需先展开")),
    "douyin" if url.host_matches("douyin.com") || url.host_matches("iesdouyin.com") => url
      .segment_after("video")
      .filter(|value| is_ascii_digits(value))
      .map(|value| normalized(AccountSourceParamKey::ItemId, value))
      .ok_or_else(|| validation_error("抖音作品链接必须包含可识别的数字 video ID；短链接需先展开")),
    "xiaohongshu" if url.host_matches("xiaohongshu.com") => url
      .segment_after("explore")
      .or_else(|| url.segment_after("item"))
      .filter(|value| is_safe_token(value))
      .map(|value| normalized(AccountSourceParamKey::ItemId, value))
      .ok_or_else(|| validation_error("小红书笔记链接缺少可识别的 note ID")),
    "xiaohongshu" if url.host_matches("xhslink.com") => {
      Ok(normalized(AccountSourceParamKey::ShareText, input))
    }
    "tiktok" | "douyin" => Err(validation_error(
      "当前链接不能在本地确定作品 ID；请使用包含作品 ID 的公开链接或直接输入 ID",
    )),
    "xiaohongshu" => Err(validation_error("请输入小红书公开笔记链接或 note ID")),
    _ => Err(validation_error("账号来源平台不受支持")),
  }
}

fn normalize_account_input(
  platform: &str,
  account_source: &str,
  input: &str,
) -> AppResult<NormalizedAccountSourceInput> {
  let parsed_url = ParsedUrl::parse(input)?;
  match (platform, parsed_url) {
    ("tiktok", Some(url)) if url.host_matches("tiktok.com") => {
      let username = url
        .segments
        .iter()
        .find_map(|segment| segment.strip_prefix('@'))
        .filter(|value| is_safe_token(value));
      if account_source == "direct_account" {
        return username
          .map(|value| normalized(AccountSourceParamKey::AccountId, value))
          .ok_or_else(|| validation_error("TikTok 主页链接缺少可识别的用户名"));
      }
      Err(validation_error(
        "该 TikTok 来源要求 user_id 或 sec_user_id，主页用户名链接无法直接使用",
      ))
    }
    ("douyin", Some(url)) if url.host_matches("douyin.com") => url
      .segment_after("user")
      .filter(|value| is_safe_token(value))
      .map(|value| normalized(AccountSourceParamKey::AccountId, value))
      .ok_or_else(|| validation_error("抖音主页链接缺少可识别的 sec_user_id；短链接需先展开")),
    ("xiaohongshu", Some(url)) if url.host_matches("xiaohongshu.com") => url
      .segment_after("profile")
      .filter(|value| is_safe_token(value))
      .map(|value| normalized(AccountSourceParamKey::AccountId, value))
      .ok_or_else(|| validation_error("小红书主页链接缺少可识别的 user_id")),
    ("xiaohongshu", Some(url)) if url.host_matches("xhslink.com") => {
      Ok(normalized(AccountSourceParamKey::ShareText, input))
    }
    ("tiktok" | "douyin", Some(_)) => Err(validation_error(
      "当前链接不能在本地确定接口所需账号标识；请使用公开主页链接或直接输入接口 ID",
    )),
    ("xiaohongshu", Some(_)) => Err(validation_error("请输入小红书公开主页链接或 user_id")),
    ("tiktok", None) => normalize_raw_tiktok_account(account_source, input),
    ("douyin", None) => {
      if is_safe_token(input) && !is_ascii_digits(input) {
        Ok(normalized(AccountSourceParamKey::AccountId, input))
      } else {
        Err(validation_error("抖音账号来源要求明确的 sec_user_id"))
      }
    }
    ("xiaohongshu", None) if is_safe_token(input) => {
      Ok(normalized(AccountSourceParamKey::AccountId, input))
    }
    _ => Err(validation_error("账号标识格式无效")),
  }
}

fn normalize_raw_tiktok_account(
  account_source: &str,
  input: &str,
) -> AppResult<NormalizedAccountSourceInput> {
  let value = input.strip_prefix('@').unwrap_or(input);
  if !is_safe_token(value) {
    return Err(validation_error("TikTok 账号标识格式无效"));
  }
  if account_source == "direct_account" || is_ascii_digits(value) || value.starts_with("MS4") {
    return Ok(normalized(AccountSourceParamKey::AccountId, value));
  }
  Err(validation_error(
    "该 TikTok 来源要求 user_id 或 sec_user_id，不能只提供用户名",
  ))
}

fn validate_raw_item_id(platform: &str, input: &str) -> AppResult<NormalizedAccountSourceInput> {
  let valid = match platform {
    "tiktok" | "douyin" => is_ascii_digits(input),
    "xiaohongshu" => is_safe_token(input),
    _ => false,
  };
  valid
    .then(|| normalized(AccountSourceParamKey::ItemId, input))
    .ok_or_else(|| validation_error("作品、视频或笔记 ID 格式无效"))
}

fn normalized(key: AccountSourceParamKey, value: &str) -> NormalizedAccountSourceInput {
  NormalizedAccountSourceInput {
    key,
    value: value.to_string(),
  }
}

fn is_ascii_digits(value: &str) -> bool {
  !value.is_empty() && value.chars().all(|character| character.is_ascii_digit())
}

fn is_safe_token(value: &str) -> bool {
  !value.is_empty()
    && value.len() <= 512
    && value
      .chars()
      .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.'))
}

struct ParsedUrl<'a> {
  host: String,
  segments: Vec<&'a str>,
}

impl<'a> ParsedUrl<'a> {
  fn parse(value: &'a str) -> AppResult<Option<Self>> {
    let Some(rest) = value
      .strip_prefix("https://")
      .or_else(|| value.strip_prefix("http://"))
    else {
      return Ok(None);
    };
    let (authority, path) = rest.split_once('/').unwrap_or((rest, ""));
    if authority.is_empty() || authority.contains('@') {
      return Err(validation_error("链接主机无效"));
    }
    let host = authority
      .split(':')
      .next()
      .unwrap_or_default()
      .trim_end_matches('.')
      .to_ascii_lowercase();
    let path = path.split(['?', '#']).next().unwrap_or_default();
    Ok(Some(Self {
      host,
      segments: path.split('/').filter(|segment| !segment.is_empty()).collect(),
    }))
  }

  fn host_matches(&self, expected: &str) -> bool {
    self.host == expected || self.host.ends_with(&format!(".{expected}"))
  }

  fn segment_after(&self, marker: &str) -> Option<&'a str> {
    self
      .segments
      .windows(2)
      .find(|segments| segments[0] == marker)
      .map(|segments| segments[1])
  }
}

fn validation_error(message: impl Into<String>) -> AppError {
  AppError::validation(message, AppErrorStage::Collection)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn normalizes_canonical_account_and_item_links() {
    let cases = [
      ("tiktok", "direct_account", AccountSourceInputKind::Account, "https://www.tiktok.com/@openai", AccountSourceParamKey::AccountId, "openai"),
      ("douyin", "direct_account", AccountSourceInputKind::Account, "https://www.douyin.com/user/MS4wLjAB-test", AccountSourceParamKey::AccountId, "MS4wLjAB-test"),
      ("xiaohongshu", "direct_account", AccountSourceInputKind::Account, "https://www.xiaohongshu.com/user/profile/abc123", AccountSourceParamKey::AccountId, "abc123"),
      ("tiktok", "item_author", AccountSourceInputKind::Item, "https://www.tiktok.com/@user/video/123456", AccountSourceParamKey::ItemId, "123456"),
      ("douyin", "comment_authors", AccountSourceInputKind::Item, "https://www.douyin.com/video/987654", AccountSourceParamKey::ItemId, "987654"),
      ("xiaohongshu", "item_author", AccountSourceInputKind::Item, "https://www.xiaohongshu.com/explore/665f95200000000006005624", AccountSourceParamKey::ItemId, "665f95200000000006005624"),
    ];
    for (platform, source, kind, input, key, value) in cases {
      assert_eq!(
        normalize_account_source_input(platform, source, kind, input).unwrap(),
        NormalizedAccountSourceInput { key, value: value.to_string() }
      );
    }
  }

  #[test]
  fn preserves_supported_xiaohongshu_share_links_and_rejects_ambiguous_short_links() {
    let share = "https://xhslink.com/m/3ZSCJZAMz0a";
    assert_eq!(
      normalize_account_source_input("xiaohongshu", "direct_account", AccountSourceInputKind::Account, share).unwrap(),
      NormalizedAccountSourceInput { key: AccountSourceParamKey::ShareText, value: share.to_string() }
    );
    assert!(normalize_account_source_input(
      "douyin",
      "item_author",
      AccountSourceInputKind::Item,
      "https://v.douyin.com/abc123/",
    ).is_err());
    assert!(normalize_account_source_input(
      "tiktok",
      "followers",
      AccountSourceInputKind::Account,
      "https://www.tiktok.com/@openai",
    ).is_err());
  }
}
