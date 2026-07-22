use crate::accounts::normalize_country_region;

pub(crate) fn valid_query_locale(value: &str) -> bool {
  let mut segments = value.split('-');
  let (Some(language), Some(region)) = (segments.next(), segments.next()) else {
    return false;
  };
  segments.next().is_none()
    && (2..=3).contains(&language.len())
    && language.chars().all(|value| value.is_ascii_lowercase())
    && region.len() == 2
    && region.chars().all(|value| value.is_ascii_uppercase())
    && normalize_country_region(Some(region)).is_some()
}

pub(crate) fn primary_query_locale(region: &str) -> Option<&'static str> {
  Some(match region {
    "GB" => "en-GB",
    "US" => "en-US",
    "CA" => "en-CA",
    "AU" => "en-AU",
    "NZ" => "en-NZ",
    "IE" => "en-IE",
    "SG" => "en-SG",
    "ZA" => "en-ZA",
    "NG" => "en-NG",
    "KE" => "en-KE",
    "CN" => "zh-CN",
    "HK" => "zh-HK",
    "MO" => "zh-MO",
    "TW" => "zh-TW",
    "JP" => "ja-JP",
    "KR" => "ko-KR",
    "FR" => "fr-FR",
    "DE" => "de-DE",
    "AT" => "de-AT",
    "CH" => "de-CH",
    "ES" => "es-ES",
    "MX" => "es-MX",
    "AR" => "es-AR",
    "CL" => "es-CL",
    "CO" => "es-CO",
    "PE" => "es-PE",
    "IT" => "it-IT",
    "PT" => "pt-PT",
    "BR" => "pt-BR",
    "NL" => "nl-NL",
    "BE" => "nl-BE",
    "SE" => "sv-SE",
    "NO" => "no-NO",
    "DK" => "da-DK",
    "FI" => "fi-FI",
    "PL" => "pl-PL",
    "CZ" => "cs-CZ",
    "GR" => "el-GR",
    "RO" => "ro-RO",
    "HU" => "hu-HU",
    "BG" => "bg-BG",
    "UA" => "uk-UA",
    "RU" => "ru-RU",
    "TR" => "tr-TR",
    "IL" => "he-IL",
    "SA" => "ar-SA",
    "AE" => "ar-AE",
    "EG" => "ar-EG",
    "MA" => "ar-MA",
    "IN" => "hi-IN",
    "PK" => "ur-PK",
    "BD" => "bn-BD",
    "TH" => "th-TH",
    "VN" => "vi-VN",
    "ID" => "id-ID",
    "MY" => "ms-MY",
    "PH" => "fil-PH",
    _ => return None,
  })
}

pub(crate) fn query_matches_locale_script(query_locale: &str, query: &str) -> bool {
  let language = query_locale.split('-').next().unwrap_or_default();
  match language {
    "zh" => matches_primary_script(query, is_han),
    "ja" => matches_primary_script(query, |value| is_han(value) || is_japanese_kana(value)),
    "ko" => matches_primary_script(query, is_hangul),
    "ru" | "uk" | "bg" => matches_primary_script(query, is_cyrillic),
    "ar" | "ur" => matches_primary_script(query, is_arabic),
    "el" => matches_primary_script(query, is_greek),
    "he" => matches_primary_script(query, is_hebrew),
    "hi" => matches_primary_script(query, is_devanagari),
    "bn" => matches_primary_script(query, is_bengali),
    "th" => matches_primary_script(query, is_thai),
    language if is_latin_query_language(language) => matches_latin_script(query),
    _ => false,
  }
}

fn matches_primary_script(value: &str, expected: impl Fn(char) -> bool) -> bool {
  let mut found = false;
  for character in value.chars().filter(|value| value.is_alphabetic()) {
    if expected(character) {
      found = true;
    } else if !is_latin(character) {
      return false;
    }
  }
  found
}

fn matches_latin_script(value: &str) -> bool {
  let mut found = false;
  for character in value.chars().filter(|value| value.is_alphabetic()) {
    if !is_latin(character) {
      return false;
    }
    found = true;
  }
  found
}

fn is_latin_query_language(value: &str) -> bool {
  matches!(
    value,
    "en"
      | "fr"
      | "de"
      | "es"
      | "it"
      | "pt"
      | "nl"
      | "sv"
      | "no"
      | "da"
      | "fi"
      | "pl"
      | "cs"
      | "ro"
      | "hu"
      | "tr"
      | "vi"
      | "id"
      | "ms"
      | "fil"
  )
}

fn is_latin(value: char) -> bool {
  matches!(value as u32, 0x0041..=0x005A | 0x0061..=0x007A | 0x00C0..=0x00D6
    | 0x00D8..=0x00F6 | 0x00F8..=0x02AF | 0x1D00..=0x1D7F | 0x1D80..=0x1DBF
    | 0x1E00..=0x1EFF | 0x2C60..=0x2C7F | 0xA720..=0xA7FF | 0xAB30..=0xAB6F
    | 0xFB00..=0xFB06 | 0xFF21..=0xFF3A | 0xFF41..=0xFF5A)
}

fn is_han(value: char) -> bool {
  matches!(value as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF
    | 0x20000..=0x2A6DF | 0x2A700..=0x2EE5F | 0x30000..=0x323AF)
}
fn is_japanese_kana(value: char) -> bool {
  matches!(value as u32, 0x3040..=0x30FF | 0x31F0..=0x31FF | 0xFF66..=0xFF9D)
}
fn is_hangul(value: char) -> bool {
  matches!(value as u32, 0x1100..=0x11FF | 0x3130..=0x318F | 0xA960..=0xA97F
    | 0xAC00..=0xD7AF | 0xD7B0..=0xD7FF)
}
fn is_cyrillic(value: char) -> bool {
  matches!(value as u32, 0x0400..=0x052F | 0x1C80..=0x1C8F | 0x2DE0..=0x2DFF | 0xA640..=0xA69F)
}
fn is_arabic(value: char) -> bool {
  matches!(value as u32, 0x0600..=0x06FF | 0x0750..=0x077F | 0x0870..=0x089F
    | 0x08A0..=0x08FF | 0xFB50..=0xFDFF | 0xFE70..=0xFEFF)
}
fn is_greek(value: char) -> bool {
  matches!(value as u32, 0x0370..=0x03FF | 0x1F00..=0x1FFF)
}
fn is_hebrew(value: char) -> bool {
  matches!(value as u32, 0x0590..=0x05FF | 0xFB1D..=0xFB4F)
}
fn is_devanagari(value: char) -> bool {
  matches!(value as u32, 0x0900..=0x097F | 0xA8E0..=0xA8FF)
}
fn is_bengali(value: char) -> bool {
  matches!(value as u32, 0x0980..=0x09FF)
}
fn is_thai(value: char) -> bool {
  matches!(value as u32, 0x0E00..=0x0E7F)
}
