use super::{CommunicateInput, CommunicateTool, EnvGuard, Request, Tool, json, test_ctx};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

#[tokio::test]
async fn new_worker_requests_forward_explicit_model_and_effort() {
    let _lock = crate::storage::lock_test_env();
    let dir = tempfile::tempdir().expect("private socket directory");
    let socket = dir.path().join("routing.sock");
    let _socket = EnvGuard::set("JCODE_SOCKET", &socket);
    #[allow(unused_mut)] // Windows named-pipe accept requires a mutable listener.
    let mut listener = crate::transport::Listener::bind(&socket).expect("private listener");
    for action in [
        "spawn",
        "assign_task",
        "assign_next",
        "fill_slots",
        "run_plan",
    ] {
        let input = json!({
            "action": action, "label": "contract capture", "prompt": "not executed",
            "model": "openai:gpt-6-astra", "effort": "low",
            "concurrency_limit": 1, "background": false
        });
        let capture = async {
            loop {
                let (stream, _) = listener.accept().await.expect("tool request");
                let (read, mut write) = stream.into_split();
                let mut line = String::new();
                BufReader::new(read)
                    .read_line(&mut line)
                    .await
                    .expect("request JSON");
                let request: Request = serde_json::from_str(&line).expect("wire request");
                let wire = serde_json::to_value(&request).expect("request value");
                let response = match wire["type"].as_str().expect("request type") {
                    "comm_plan_status" => {
                        json!({"type":"comm_plan_status_response", "id":request.id(),
                        "summary":{"version":0,"item_count":1,"ready_ids":["task"],"next_ready_ids":["task"]}})
                    }
                    "comm_list" => json!({"type":"comm_members", "id":request.id(), "members":[]}),
                    "comm_spawn" | "comm_assign_next" => {
                        json!({"type":"error", "id":request.id(), "message":"capture-only"})
                    }
                    other => panic!("unexpected setup request: {other}"),
                };
                write
                    .write_all(format!("{response}\n").as_bytes())
                    .await
                    .expect("capture response");
                if matches!(
                    wire["type"].as_str(),
                    Some("comm_spawn" | "comm_assign_next")
                ) {
                    break wire;
                }
            }
        };
        let call = async {
            let ctx = test_ctx("coordinator", dir.path());
            if action == "assign_task" {
                let params: CommunicateInput = serde_json::from_value(input).expect("tool input");
                super::super::spawn_assignment_session(&ctx, &params)
                    .await
                    .expect_err("capture never starts a worker");
            } else {
                CommunicateTool::new()
                    .execute(input, ctx)
                    .await
                    .expect_err("capture never starts a worker");
            }
        };
        let (wire, ()) = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            tokio::join!(capture, call)
        })
        .await
        .unwrap_or_else(|error| panic!("bounded {action} request capture: {error}"));
        assert_eq!(wire["model"], "openai:gpt-6-astra", "{action}");
        assert_eq!(wire["effort"], "low", "{action}");
        assert_eq!(
            wire["type"],
            if matches!(action, "spawn" | "assign_task") {
                "comm_spawn"
            } else {
                "comm_assign_next"
            }
        );
    }
}
