use super::*;

#[tokio::test]
async fn plugin_reconnect_reads_latest_state_without_retaining_updates() {
    let hub = Hub::default();
    let topic = Topic::keyed("plugin", "example");
    let mut changes = hub.subscribe();
    hub.publish(topic.clone(), vec![1]).unwrap();
    hub.publish(topic.clone(), vec![2]).unwrap();
    assert_eq!(changes.recv().await.unwrap().topic, topic);
    assert_eq!(hub.current(&topic), Some(Value::Plugin(vec![2])));
    assert_eq!(crate::lock(&hub.0.states).len(), 1);
    assert!(hub.publish(Topic::new("plugin"), vec![]).is_err());
}

#[tokio::test]
async fn unchanged_plugin_state_does_not_wake_readers() {
    let hub = Hub::default();
    let topic = Topic::keyed("plugin", "example");
    hub.publish(topic.clone(), vec![1]).unwrap();
    let mut changes = hub.subscribe();
    hub.publish(topic, vec![1]).unwrap();
    assert!(matches!(
        changes.try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));
}

#[cfg(feature = "host")]
#[tokio::test]
async fn a_system_change_reaches_a_hub_only_while_its_forward_is_held() {
    let hub = Hub::default();
    let mut changes = hub.subscribe();
    let topic = Topic::keyed("plugin", "forward-system-test");

    system().changed(topic.clone());
    assert!(matches!(
        changes.try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));

    let forward = hub.forward_system();
    // The task takes its subscription when it first runs.
    tokio::task::yield_now().await;
    system().changed(topic.clone());
    let heard = tokio::time::timeout(std::time::Duration::from_secs(10), changes.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(heard.topic, topic);

    drop(forward);
    tokio::task::yield_now().await;
    system().changed(topic);
    assert!(matches!(
        changes.try_recv(),
        Err(broadcast::error::TryRecvError::Empty)
    ));
}
