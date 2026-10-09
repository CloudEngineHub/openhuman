use super::*;
use serde_json::json;

fn params(message_id: &str, text: &str) -> RelayInboundParams {
    serde_json::from_value(json!({
        "channel": "telegram",
        "chat_id": "-100",
        "sender_id": "42",
        "message_id": message_id,
        "text": text,
    }))
    .expect("params")
}

#[tokio::test]
async fn an_inbound_message_is_recorded_once() {
    let tmp = tempfile::tempdir().unwrap();
    let p = params("m1", "hello");
    let thread = p.thread_id();
    assert_eq!(
        record_inbound(tmp.path(), &p, &thread).await,
        Ok(Recorded::New)
    );
    assert_eq!(
        record_inbound(tmp.path(), &p, &thread).await,
        Ok(Recorded::Duplicate),
        "a gateway retry is not recorded twice"
    );

    let threads = crate::threads::store::list_threads(tmp.path().to_path_buf()).unwrap();
    assert_eq!(threads.len(), 1);
    assert_eq!(threads[0].id, thread);
    assert_eq!(threads[0].title, "telegram · 42 · -100");
    let messages = crate::threads::store::get_messages(tmp.path().to_path_buf(), &thread).unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].id, "user:m1");
    assert_eq!(messages[0].sender, "user");
    assert_eq!(messages[0].extra_metadata["channel"], "telegram");
}

#[tokio::test]
async fn replies_are_recorded_and_seed_the_next_turn() {
    let tmp = tempfile::tempdir().unwrap();
    let first = params("m1", "hello");
    let thread = first.thread_id();
    record_inbound(tmp.path(), &first, &thread).await.unwrap();
    record_reply(tmp.path(), &first, &thread, &[])
        .await
        .expect("nothing sent, nothing recorded");
    record_reply(tmp.path(), &first, &thread, &["hi".into(), "there".into()])
        .await
        .unwrap();
    let second = params("m2", "again");
    record_inbound(tmp.path(), &second, &thread).await.unwrap();

    let prior = prior_turns(tmp.path(), &thread, "m2", 10).await.unwrap();
    let rows: Vec<(String, String)> = prior
        .iter()
        .map(|m| (m.role.clone(), m.content.clone()))
        .collect();
    assert_eq!(
        rows,
        vec![
            ("user".to_string(), "hello".to_string()),
            ("assistant".to_string(), "hi\n\nthere".to_string()),
        ],
        "earlier turns only, not the message being answered"
    );
    let limited = prior_turns(tmp.path(), &thread, "m2", 1).await.unwrap();
    assert_eq!(limited.len(), 1);
    assert_eq!(limited[0].content, "hi\n\nthere", "the most recent rows");
}
