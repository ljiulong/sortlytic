use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::domain::{AppError, AppErrorStage, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgeRange {
  pub min: i64,
  pub max: i64,
}

impl AgeRange {
  pub fn includes(self, age: Option<i64>) -> bool {
    age.is_some_and(|age| {
      (0..=130).contains(&age) && self.min <= age && age <= self.max && self.min <= self.max
    })
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
  CommentAuthor,
  ContentAuthor,
  AccountProfile,
}

impl SourceKind {
  fn priority(self) -> u8 {
    match self {
      Self::CommentAuthor => 1,
      Self::ContentAuthor => 2,
      Self::AccountProfile => 3,
    }
  }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AccountRecord {
  pub platform: String,
  pub identity_key: String,
  pub platform_user_id: Option<String>,
  pub username: Option<String>,
  pub account: Option<String>,
  pub profile_text: Option<String>,
  pub country_region: Option<String>,
  pub gender: Option<String>,
  pub age: Option<i64>,
  pub followers_count: Option<i64>,
  pub posts_count: Option<i64>,
  pub last_posted_at: Option<String>,
  pub profile_url: Option<String>,
  #[serde(skip)]
  field_priorities: BTreeMap<String, u8>,
}

#[derive(Debug, Default)]
pub struct AccountAccumulator {
  accounts: BTreeMap<String, AccountRecord>,
}

impl AccountAccumulator {
  pub fn upsert(&mut self, incoming: AccountRecord) {
    let key = format!("{}:{}", incoming.platform, incoming.identity_key);
    match self.accounts.get_mut(&key) {
      Some(existing) => merge_account(existing, incoming),
      None => {
        self.accounts.insert(key, incoming);
      }
    }
  }

  pub fn into_filtered(self, age_range: Option<AgeRange>, limit: usize) -> Vec<AccountRecord> {
    self
      .accounts
      .into_values()
      .filter(|account| age_range.is_none_or(|range| range.includes(account.age)))
      .take(limit)
      .collect()
  }
}

pub fn normalize_account(
  platform: &str,
  source_kind: SourceKind,
  value: &Value,
) -> AppResult<AccountRecord> {
  let platform = match platform.trim() {
    "tiktok" | "douyin" | "xiaohongshu" => platform.trim().to_string(),
    _ => {
      return Err(AppError::validation(
        "账号归一化只支持 TikTok、抖音、小红书",
        AppErrorStage::Collection,
      ));
    }
  };
  let platform_user_id = first_text(
    value,
    &[
      "/platform_user_id",
      "/sec_user_id",
      "/user_id",
      "/uid",
      "/author/sec_user_id",
      "/author/user_id",
      "/user/sec_user_id",
      "/user/user_id",
    ],
  );
  let account = first_text(
    value,
    &[
      "/unique_id",
      "/account",
      "/username",
      "/author/unique_id",
      "/author/username",
      "/user/unique_id",
      "/user/username",
    ],
  );
  let identity_key = platform_user_id
    .as_ref()
    .map(|value| format!("id:{value}"))
    .or_else(|| {
      account
        .as_deref()
        .map(normalized_account_name)
        .filter(|value| !value.is_empty())
        .map(|value| format!("account:{value}"))
    })
    .ok_or_else(|| {
      AppError::validation(
        "账号记录缺少平台用户 ID 和可用账号名",
        AppErrorStage::Collection,
      )
    })?;
  let priority = source_kind.priority();
  let mut field_priorities = BTreeMap::new();
  for field in [
    "platform_user_id",
    "username",
    "account",
    "profile_text",
    "country_region",
    "gender",
    "age",
    "followers_count",
    "posts_count",
    "last_posted_at",
    "profile_url",
  ] {
    field_priorities.insert(field.to_string(), priority);
  }

  Ok(AccountRecord {
    platform,
    identity_key,
    platform_user_id,
    username: first_text(
      value,
      &[
        "/nickname",
        "/display_name",
        "/name",
        "/author/nickname",
        "/user/nickname",
      ],
    ),
    account,
    profile_text: first_text(
      value,
      &[
        "/signature",
        "/bio",
        "/desc",
        "/author/signature",
        "/user/signature",
      ],
    ),
    country_region: first_text(
      value,
      &[
        "/region",
        "/country",
        "/country_code",
        "/author/region",
        "/user/region",
      ],
    ),
    gender: first_text(
      value,
      &["/gender", "/sex", "/author/gender", "/user/gender"],
    ),
    age: first_age(value),
    followers_count: first_nonnegative_integer(
      value,
      &[
        "/followers_count",
        "/follower_count",
        "/fans",
        "/author/fans",
        "/user/fans",
      ],
    ),
    posts_count: first_nonnegative_integer(
      value,
      &[
        "/posts_count",
        "/aweme_count",
        "/note_count",
        "/author/aweme_count",
        "/user/aweme_count",
      ],
    ),
    last_posted_at: first_text(value, &["/last_posted_at", "/latest_posted_at"]),
    profile_url: first_text(
      value,
      &[
        "/profile_url",
        "/share_url",
        "/author/share_url",
        "/user/share_url",
      ],
    ),
    field_priorities,
  })
}

fn merge_account(existing: &mut AccountRecord, incoming: AccountRecord) {
  merge_field(existing, &incoming, "platform_user_id", |record| {
    &mut record.platform_user_id
  });
  merge_field(existing, &incoming, "username", |record| {
    &mut record.username
  });
  merge_field(existing, &incoming, "account", |record| &mut record.account);
  merge_field(existing, &incoming, "profile_text", |record| {
    &mut record.profile_text
  });
  merge_field(existing, &incoming, "country_region", |record| {
    &mut record.country_region
  });
  merge_field(existing, &incoming, "gender", |record| &mut record.gender);
  merge_copy_field(existing, &incoming, "age", |record| &mut record.age);
  merge_copy_field(existing, &incoming, "followers_count", |record| {
    &mut record.followers_count
  });
  merge_copy_field(existing, &incoming, "posts_count", |record| {
    &mut record.posts_count
  });
  merge_field(existing, &incoming, "last_posted_at", |record| {
    &mut record.last_posted_at
  });
  merge_field(existing, &incoming, "profile_url", |record| {
    &mut record.profile_url
  });
}

fn merge_field(
  existing: &mut AccountRecord,
  incoming: &AccountRecord,
  field: &str,
  select: impl Fn(&mut AccountRecord) -> &mut Option<String> + Copy,
) {
  let incoming_value = select(&mut incoming.clone()).clone();
  merge_value(existing, incoming, field, incoming_value, select);
}

fn merge_copy_field(
  existing: &mut AccountRecord,
  incoming: &AccountRecord,
  field: &str,
  select: impl Fn(&mut AccountRecord) -> &mut Option<i64> + Copy,
) {
  let incoming_value = *select(&mut incoming.clone());
  merge_value(existing, incoming, field, incoming_value, select);
}

fn merge_value<T: Clone>(
  existing: &mut AccountRecord,
  incoming: &AccountRecord,
  field: &str,
  incoming_value: Option<T>,
  select: impl Fn(&mut AccountRecord) -> &mut Option<T>,
) {
  let incoming_priority = incoming.field_priorities.get(field).copied().unwrap_or(0);
  let existing_priority = existing.field_priorities.get(field).copied().unwrap_or(0);
  if incoming_value.is_some()
    && (select(existing).is_none() || incoming_priority > existing_priority)
  {
    *select(existing) = incoming_value;
    existing
      .field_priorities
      .insert(field.to_string(), incoming_priority);
  }
}

fn first_age(value: &Value) -> Option<i64> {
  ["/age", "/author/age", "/user/age", "/user_info/age"]
    .iter()
    .find_map(|path| value.pointer(path))
    .and_then(|value| {
      value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
    })
    .filter(|age| (0..=130).contains(age))
}

fn first_nonnegative_integer(value: &Value, paths: &[&str]) -> Option<i64> {
  paths
    .iter()
    .filter_map(|path| value.pointer(path))
    .find_map(|value| {
      value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
    })
    .filter(|value| *value >= 0)
}

fn first_text(value: &Value, paths: &[&str]) -> Option<String> {
  paths
    .iter()
    .filter_map(|path| value.pointer(path))
    .find_map(|value| {
      value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string)
    })
}

fn normalized_account_name(value: &str) -> String {
  value
    .chars()
    .filter(|character| !character.is_whitespace() && *character != '@')
    .flat_map(char::to_lowercase)
    .collect()
}

#[cfg(test)]
#[path = "accounts_tests.rs"]
mod tests;
