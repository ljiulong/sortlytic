use std::cell::RefCell;

use super::*;

thread_local! {
  static BASE_URL_OVERRIDE: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub(crate) struct TestBaseUrlOverrideGuard;

impl Drop for TestBaseUrlOverrideGuard {
  fn drop(&mut self) {
    BASE_URL_OVERRIDE.with(|slot| *slot.borrow_mut() = None);
  }
}

pub(crate) fn override_tikhub_base_url_for_current_test(
  base_url: String,
) -> TestBaseUrlOverrideGuard {
  assert!(base_url.starts_with("http://127.0.0.1:"));
  BASE_URL_OVERRIDE.with(|slot| {
    assert!(slot.borrow().is_none());
    *slot.borrow_mut() = Some(base_url);
  });
  TestBaseUrlOverrideGuard
}

pub(super) fn overridden_base_url(base_url: &str) -> Option<String> {
  let overridden = BASE_URL_OVERRIDE.with(|slot| slot.borrow().clone())?;
  (matches!(base_url, DEFAULT_BASE_URL | CHINA_BASE_URL) || base_url == overridden)
    .then_some(overridden)
}

#[path = "collection_tests.rs"]
mod collection_tests;

#[path = "business_response_tests.rs"]
mod business_response_tests;
