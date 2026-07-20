use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ModelCallPurpose {
  ConnectionTest,
  CollectionIntent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ModelTimeouts {
  pub connect: Duration,
  pub total: Duration,
}

pub(super) fn model_timeouts(purpose: ModelCallPurpose) -> ModelTimeouts {
  ModelTimeouts {
    connect: Duration::from_secs(10),
    total: Duration::from_secs(match purpose {
      ModelCallPurpose::ConnectionTest => 30,
      ModelCallPurpose::CollectionIntent => 90,
    }),
  }
}

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use super::{model_timeouts, ModelCallPurpose};

  #[test]
  fn keeps_connection_tests_short_and_allows_intent_generation_more_time() {
    let connection_test = model_timeouts(ModelCallPurpose::ConnectionTest);
    let collection_intent = model_timeouts(ModelCallPurpose::CollectionIntent);

    assert_eq!(connection_test.connect, Duration::from_secs(10));
    assert_eq!(connection_test.total, Duration::from_secs(30));
    assert_eq!(collection_intent.connect, Duration::from_secs(10));
    assert_eq!(collection_intent.total, Duration::from_secs(90));
  }
}
