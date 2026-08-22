#[tokio::test]
async fn fire_before_notified_does_not_hang() {
    let sig = InterruptSignal::new();
    sig.fire();
    tokio::time::timeout(std::time::Duration::from_millis(100), sig.notified())
        .await
        .expect("notified() hung when signal was already set before call");
}

#[tokio::test]
async fn fire_concurrent_with_notified() {
    let sig = Arc::new(InterruptSignal::new());
    let sig2 = Arc::clone(&sig);
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        sig2.fire();
    });

    tokio::time::timeout(std::time::Duration::from_millis(500), sig.notified())
        .await
        .expect("notified() hung during concurrent fire()");
}

#[tokio::test]
async fn is_set_false_initially() {
    assert!(!InterruptSignal::new().is_set());
}

#[tokio::test]
async fn is_set_true_after_fire() {
    let sig = InterruptSignal::new();
    sig.fire();
    assert!(sig.is_set());
}

#[tokio::test]
async fn reset_clears_flag() {
    let sig = InterruptSignal::new();
    sig.fire();
    assert!(sig.is_set());
    sig.reset();
    assert!(!sig.is_set());
}

#[tokio::test]
async fn notified_completes_after_fire() {
    let sig = Arc::new(InterruptSignal::new());
    let sig2 = Arc::clone(&sig);
    let handle = tokio::spawn(async move { sig2.notified().await });

    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    sig.fire();

    tokio::time::timeout(std::time::Duration::from_millis(200), handle)
        .await
        .expect("notified() task timed out after fire()")
        .expect("task panicked");
}
