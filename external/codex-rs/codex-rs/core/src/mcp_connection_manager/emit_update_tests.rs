use super::Event;
use super::EventMsg;
use super::INITIAL_SUBMIT_ID;
use super::McpStartupStatus;
use super::McpStartupUpdateEvent;
use super::emit_update;
use pretty_assertions::assert_eq;

// Protects: a startup update reaches its receiver completely and exactly once.
// Detects: dropping, duplicating or altering the event while changing send-error storage.
// Needs: Tokio and one owned in-process channel; no server, model, device or live bus.
// Breadcrumb: external/codex-rs/codex-rs/core/src/mcp_connection_manager.rs (emit_update).
#[tokio::test]
async fn emit_update_delivers_complete_event_once() {
    let (tx_event, rx_event) = async_channel::bounded(1);
    let update = McpStartupUpdateEvent {
        server: "test-server".to_string(),
        status: McpStartupStatus::Ready,
    };
    let expected = Event {
        id: INITIAL_SUBMIT_ID.to_owned(),
        msg: EventMsg::McpStartupUpdate(update.clone()),
    };

    emit_update(&tx_event, update).await.unwrap();

    let delivered = rx_event.try_recv().unwrap();
    assert_eq!(
        serde_json::to_value(delivered).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(matches!(
        rx_event.try_recv(),
        Err(async_channel::TryRecvError::Empty)
    ));
}

// Protects: send failure retains the original unsent startup status and its payload.
// Detects: replacing the concrete channel error with a lossy error during boxing.
// Needs: Tokio and one owned closed channel; no server, model, device or live bus.
// Breadcrumb: external/codex-rs/codex-rs/core/src/mcp_connection_manager.rs (emit_update).
#[tokio::test]
async fn emit_update_preserves_unsent_event_when_receiver_is_closed() {
    let (tx_event, rx_event) = async_channel::bounded(1);
    rx_event.close();
    let update = McpStartupUpdateEvent {
        server: "test-server".to_string(),
        status: McpStartupStatus::Failed {
            error: "startup failed".to_string(),
        },
    };
    let expected = Event {
        id: INITIAL_SUBMIT_ID.to_owned(),
        msg: EventMsg::McpStartupUpdate(update.clone()),
    };

    let error = emit_update(&tx_event, update).await.unwrap_err();
    let unsent = error.0;

    assert_eq!(
        serde_json::to_value(unsent).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(matches!(
        rx_event.try_recv(),
        Err(async_channel::TryRecvError::Closed)
    ));
}
