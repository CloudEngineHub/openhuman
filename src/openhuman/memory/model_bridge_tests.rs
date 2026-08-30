use super::*;

/// `tinyagents`' `harness::model::ModelResponse` and `tinyinference::model::
/// ModelResponse` are wire-identical (see module docs): a value built on one
/// side must survive a round trip through the other and back with every
/// field intact.
#[test]
fn model_response_round_trips_between_legacy_and_current_types() {
    let current = tinyinference::model::ModelResponse::assistant("hello from the bridge test");

    let legacy: tinyagents::harness::model::ModelResponse =
        bridge_via_json(&current).expect("current -> legacy response should bridge");
    assert_eq!(legacy.message.content.len(), current.message.content.len());

    let back: tinyinference::model::ModelResponse =
        bridge_response_to_new(&legacy).expect("legacy -> current response should bridge");
    assert_eq!(back.text(), current.text());
}

#[test]
fn model_profile_round_trips_between_legacy_and_current_types() {
    let current = tinyinference::model::ModelProfile::permissive();
    let legacy = bridge_profile_to_old(&current).expect("profile should bridge to the legacy type");
    assert_eq!(legacy.tool_calling, current.tool_calling);
    assert_eq!(legacy.streaming, current.streaming);
}

#[tokio::test]
async fn legacy_bridge_invokes_the_wrapped_current_model() {
    use async_trait::async_trait;

    struct EchoModel;

    #[async_trait]
    impl tinyinference::model::ChatModel<()> for EchoModel {
        async fn invoke(
            &self,
            _state: &(),
            _request: tinyinference::model::ModelRequest,
        ) -> tinyinference::Result<tinyinference::model::ModelResponse> {
            Ok(tinyinference::model::ModelResponse::assistant("bridged reply"))
        }
    }

    let bridge = LegacyChatModelBridge::new(std::sync::Arc::new(EchoModel));
    let request = tinyagents::harness::model::ModelRequest::new(vec![
        tinyagents::harness::message::Message::user("hi"),
    ]);
    let response = tinyagents::harness::model::ChatModel::invoke(&bridge, &(), request)
        .await
        .expect("bridged invoke should succeed");
    assert_eq!(response.message.content.len(), 1);
}
