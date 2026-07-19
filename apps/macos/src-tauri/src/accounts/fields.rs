use std::collections::BTreeMap;

use serde_json::Value;

use super::FieldEvidence;

pub(super) struct FieldExtraction {
  pub(super) values: BTreeMap<String, Value>,
  pub(super) evidence: BTreeMap<String, FieldEvidence>,
}

pub(super) fn extract_account_fields(
  value: &Value,
  endpoint_key: &str,
  collected_at: &str,
) -> FieldExtraction {
  let mut extraction = FieldExtraction {
    values: BTreeMap::new(),
    evidence: BTreeMap::new(),
  };

  if endpoint_key.ends_with(".account_posts") {
    add(
      &mut extraction,
      "last_posted_at",
      timestamp(
        value,
        &[
          "/last_posted_at",
          "/latest_posted_at",
          "/create_time",
          "/publish_time",
          "/note_card/time",
          "/time",
        ],
      ),
      endpoint_key,
      collected_at,
    );
    return extraction;
  }

  add(
    &mut extraction,
    "secure_user_id",
    text(
      value,
      &[
        "/secure_user_id",
        "/sec_user_id",
        "/sec_uid",
        "/author/sec_uid",
        "/user/sec_uid",
      ],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "avatar_url",
    text(
      value,
      &[
        "/avatar_url",
        "/avatar_larger/url_list/0",
        "/avatar_medium/url_list/0",
        "/avatar_thumb/url_list/0",
        "/images",
        "/user/avatar",
        "/author/avatar",
      ],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "profile_url",
    text(
      value,
      &[
        "/profile_url",
        "/share_url",
        "/share_info/share_url",
        "/user/share_url",
      ],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "bio",
    text(value, &["/bio", "/signature", "/desc", "/user/signature"]),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "website_url",
    text(
      value,
      &["/website_url", "/bio_link/link", "/bio_url", "/url"],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "verification_status",
    boolean(
      value,
      &[
        "/verification_status",
        "/verified",
        "/is_verified",
        "/is_star",
      ],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "verification_reason",
    text(
      value,
      &[
        "/verification_reason",
        "/enterprise_verify_reason",
        "/custom_verify",
        "/verify_info",
      ],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "account_type",
    scalar(value, &["/account_type", "/user_type", "/type"]),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "private_account",
    boolean(value, &["/private_account", "/is_private", "/secret"]),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "language",
    text(value, &["/language", "/language_code", "/user/language"]),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "country_region",
    text(
      value,
      &["/country_region", "/country", "/region", "/country_code"],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "profile_tags",
    list(value, &["/profile_tags", "/tags", "/tag_list"]),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "gender",
    gender(value),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "age",
    age(value),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "followers_count",
    nonnegative_integer(
      value,
      &[
        "/followers_count",
        "/follower_count",
        "/fans",
        "/fans_count",
      ],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "following_count",
    nonnegative_integer(value, &["/following_count", "/follow_count", "/follows"]),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "friends_count",
    nonnegative_integer(value, &["/friends_count", "/friend_count"]),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "posts_count",
    nonnegative_integer(
      value,
      &[
        "/posts_count",
        "/aweme_count",
        "/note_count",
        "/notes_count",
      ],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "likes_received_count",
    nonnegative_integer(
      value,
      &[
        "/likes_received_count",
        "/total_favorited",
        "/liked_count",
        "/interaction_info/liked_count",
      ],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "liked_content_count",
    nonnegative_integer(
      value,
      &[
        "/liked_content_count",
        "/favoriting_count",
        "/liked_aweme_count",
      ],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "account_created_at",
    timestamp(
      value,
      &["/account_created_at", "/create_time", "/created_at"],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "last_posted_at",
    timestamp(
      value,
      &[
        "/last_posted_at",
        "/latest_posted_at",
        "/latest_post/create_time",
      ],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "live_status",
    boolean(value, &["/live_status", "/is_live", "/room_data/status"]),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "live_room_id",
    scalar(
      value,
      &[
        "/live_room_id",
        "/room_id",
        "/room_data/id",
        "/live_user/room_id",
      ],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "username_modified_at",
    timestamp(
      value,
      &[
        "/username_modified_at",
        "/username_modify_time",
        "/unique_id_modify_time",
      ],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "nickname_modified_at",
    timestamp(value, &["/nickname_modified_at", "/nickname_update_time"]),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "commerce_status",
    boolean(
      value,
      &["/commerce_status", "/commerce_user", "/is_commerce_user"],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "commerce_category",
    scalar(
      value,
      &["/commerce_category", "/commerce_user_level", "/category"],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "seller_status",
    boolean(value, &["/seller_status", "/is_shop_author", "/is_seller"]),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "organization_status",
    boolean(
      value,
      &["/organization_status", "/is_organization", "/is_org"],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "comments_permission",
    scalar(
      value,
      &[
        "/comments_permission",
        "/comment_setting",
        "/comment_permission",
      ],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "duet_permission",
    scalar(value, &["/duet_permission", "/duet_setting"]),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "stitch_permission",
    scalar(value, &["/stitch_permission", "/stitch_setting"]),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "download_permission",
    scalar(value, &["/download_permission", "/download_setting"]),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "favorites_visibility",
    scalar(value, &["/favorites_visibility", "/favorite_permission"]),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "following_visibility",
    scalar(
      value,
      &["/following_visibility", "/following_visibility_status"],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "playlist_visibility",
    scalar(value, &["/playlist_visibility", "/playlist_permission"]),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "live_level",
    nonnegative_integer(
      value,
      &[
        "/live_level",
        "/live_user/level",
        "/live_user/pay_grade/level",
      ],
    ),
    endpoint_key,
    collected_at,
  );
  add(
    &mut extraction,
    "live_badge",
    text(
      value,
      &[
        "/live_badge",
        "/live_user/badge",
        "/live_user/pay_grade/name",
      ],
    ),
    endpoint_key,
    collected_at,
  );
  extraction
}

fn add(
  extraction: &mut FieldExtraction,
  key: &str,
  found: Option<(Value, &'static str)>,
  endpoint_key: &str,
  collected_at: &str,
) {
  let Some((value, path)) = found else {
    return;
  };
  extraction.values.insert(key.to_string(), value);
  extraction.evidence.insert(
    key.to_string(),
    FieldEvidence {
      endpoint_key: endpoint_key.to_string(),
      raw_path: path.to_string(),
      collected_at: collected_at.to_string(),
    },
  );
}

fn text(value: &Value, paths: &[&'static str]) -> Option<(Value, &'static str)> {
  paths.iter().find_map(|path| {
    value.pointer(path).and_then(|value| {
      value
        .as_str()
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(|text| (Value::String(text.to_string()), *path))
    })
  })
}

fn scalar(value: &Value, paths: &[&'static str]) -> Option<(Value, &'static str)> {
  paths.iter().find_map(|path| {
    value.pointer(path).and_then(|value| match value {
      Value::String(text) if !text.trim().is_empty() => {
        Some((Value::String(text.trim().to_string()), *path))
      }
      Value::Bool(_) | Value::Number(_) => Some((value.clone(), *path)),
      _ => None,
    })
  })
}

fn boolean(value: &Value, paths: &[&'static str]) -> Option<(Value, &'static str)> {
  paths.iter().find_map(|path| {
    let value = value.pointer(path)?;
    let normalized = value.as_bool().or_else(|| match value.as_i64() {
      Some(0) => Some(false),
      Some(1) => Some(true),
      _ => match value.as_str()?.trim().to_ascii_lowercase().as_str() {
        "false" | "0" => Some(false),
        "true" | "1" => Some(true),
        _ => None,
      },
    })?;
    Some((Value::Bool(normalized), *path))
  })
}

fn nonnegative_integer(value: &Value, paths: &[&'static str]) -> Option<(Value, &'static str)> {
  integer(value, paths).filter(|(value, _)| value.as_i64().is_some_and(|value| value >= 0))
}

fn integer(value: &Value, paths: &[&'static str]) -> Option<(Value, &'static str)> {
  paths.iter().find_map(|path| {
    let value = value.pointer(path)?;
    value
      .as_i64()
      .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
      .map(|value| (Value::from(value), *path))
  })
}

fn timestamp(value: &Value, paths: &[&'static str]) -> Option<(Value, &'static str)> {
  scalar(value, paths)
}

fn list(value: &Value, paths: &[&'static str]) -> Option<(Value, &'static str)> {
  paths.iter().find_map(|path| {
    let values = value.pointer(path)?.as_array()?;
    let values = values
      .iter()
      .filter_map(|value| value.as_str().map(str::trim))
      .filter(|value| !value.is_empty())
      .map(|value| Value::String(value.to_string()))
      .collect::<Vec<_>>();
    (!values.is_empty()).then_some((Value::Array(values), *path))
  })
}

fn gender(value: &Value) -> Option<(Value, &'static str)> {
  for path in [
    "/gender",
    "/sex",
    "/author/gender",
    "/user/gender",
    "/live_user/gender",
  ] {
    let Some(candidate) = value.pointer(path) else {
      continue;
    };
    let Some(normalized) = candidate
      .as_str()
      .map(str::trim)
      .map(str::to_lowercase)
      .or_else(|| candidate.as_i64().map(|value| value.to_string()))
    else {
      continue;
    };
    let gender = match normalized.as_str() {
      "male" | "m" | "男" | "男性" | "1" => "male",
      "female" | "f" | "女" | "女性" | "2" => "female",
      "other" | "其他" | "其它" | "non_binary" | "non-binary" => "other",
      _ => continue,
    };
    return Some((Value::String(gender.to_string()), path));
  }
  None
}

fn age(value: &Value) -> Option<(Value, &'static str)> {
  integer(
    value,
    &["/age", "/author/age", "/user/age", "/user_info/age"],
  )
  .filter(|(value, _)| value.as_i64().is_some_and(|age| (0..=130).contains(&age)))
}
