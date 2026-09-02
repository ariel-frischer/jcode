#![cfg(unix)]

use jcode_harness_api::{
    API_VERSION_MAJOR, ApiEvent, ApiRequest, ClientFrame, QUEUED_MESSAGE_NAVIGATION_CAPABILITY,
    QueuedMessageEditorDirection, QueuedMessageEditorDraft, QueuedMessageEditorOperation,
    QueuedMessageEditorOutcome, QueuedMessageEditorPlacement, QueuedMessageEditorSelection,
    ServerFrame, read_frame, write_frame,
};
use jcode_sdk::{ConnectOptions, JcodeClient, Transport};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct PairTransport(UnixStream);

impl Transport for PairTransport {
    fn split(
        self: Box<Self>,
    ) -> jcode_sdk::Result<(Box<dyn BufRead + Send>, Box<dyn Write + Send>)> {
        let writer = self.0.try_clone().unwrap();
        Ok((Box::new(BufReader::new(self.0)), Box::new(writer)))
    }
}

fn client(capable: bool, requests: Arc<Mutex<Vec<ApiRequest>>>) -> JcodeClient {
    let (ours, theirs) = UnixStream::pair().unwrap();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(theirs.try_clone().unwrap());
        let mut writer = theirs;
        while let Ok(frame) = read_frame::<_, ClientFrame>(&mut reader) {
            if matches!(frame.request, ApiRequest::Hello { .. }) {
                write_frame(
                    &mut writer,
                    &ServerFrame::reply(
                        frame.id,
                        ApiEvent::HelloOk {
                            version: API_VERSION_MAJOR,
                            server: "fake".into(),
                            capabilities: capable
                                .then(|| QUEUED_MESSAGE_NAVIGATION_CAPABILITY.to_string())
                                .into_iter()
                                .collect(),
                        },
                    ),
                )
                .unwrap();
                continue;
            }
            requests.lock().unwrap().push(frame.request.clone());
            if let ApiRequest::QueuedMessageEditor {
                navigation_session_id,
                operation_id,
                ..
            } = frame.request
            {
                write_frame(
                    &mut writer,
                    &ServerFrame::reply(
                        frame.id,
                        ApiEvent::QueuedMessageEditorResult {
                            session_id: "session-1".into(),
                            navigation_session_id,
                            operation_id,
                            outcome: QueuedMessageEditorOutcome::Moved,
                            selection: Some(QueuedMessageEditorSelection {
                                message_id: "message-1".into(),
                                content: "draft".into(),
                                images: vec![
                                    ("image/png".into(), "one".into()),
                                    ("image/jpeg".into(), "two".into()),
                                ],
                                older_available: false,
                                newer_available: true,
                            }),
                            placement: QueuedMessageEditorPlacement::Exact,
                            message: None,
                        },
                    ),
                )
                .unwrap();
            }
        }
    });
    JcodeClient::connect_with(
        Box::new(PairTransport(ours)),
        ConnectOptions {
            ensure_runtime: false,
            request_timeout: Some(Duration::from_secs(2)),
            ..Default::default()
        },
    )
    .unwrap()
}

#[test]
fn legacy_peer_is_rejected_before_an_editor_request_is_written() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let client = client(false, Arc::clone(&requests));
    let error = client
        .queued_message_editor(
            "session-1",
            "navigation-1",
            "operation-1",
            QueuedMessageEditorOperation::Start,
        )
        .unwrap_err();
    assert!(error.message.contains(QUEUED_MESSAGE_NAVIGATION_CAPABILITY));
    assert!(requests.lock().unwrap().is_empty());
}

#[test]
fn capable_peer_roundtrips_typed_outcomes_and_ordered_images() {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let client = client(true, Arc::clone(&requests));
    let result = client
        .queued_message_editor(
            "session-1",
            "navigation-1",
            "operation-1",
            QueuedMessageEditorOperation::Move {
                direction: QueuedMessageEditorDirection::Older,
                selected_message_id: "message-2".into(),
                draft: QueuedMessageEditorDraft {
                    content: "saved".into(),
                    images: vec![("image/webp".into(), "saved-image".into())],
                },
            },
        )
        .unwrap();

    let ApiEvent::QueuedMessageEditorResult {
        outcome,
        selection: Some(selection),
        placement,
        ..
    } = result
    else {
        panic!("expected typed queued-message editor result")
    };
    assert_eq!(outcome, QueuedMessageEditorOutcome::Moved);
    assert_eq!(placement, QueuedMessageEditorPlacement::Exact);
    assert_eq!(selection.images[0].1, "one");
    assert_eq!(selection.images[1].1, "two");
    assert!(matches!(
        requests.lock().unwrap().as_slice(),
        [ApiRequest::QueuedMessageEditor { .. }]
    ));
}
