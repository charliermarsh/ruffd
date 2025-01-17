use ruffd_types::ruff::check;
use ruffd_types::ruff::message::Message;
use ruffd_types::tokio::sync::mpsc::Sender;
use ruffd_types::tokio::sync::Mutex;
use ruffd_types::{lsp_types, serde_json};
use ruffd_types::{
    CreateLocksFn, RpcNotification, RwGuarded, RwReq, ScheduledTask, ServerNotification,
    ServerNotificationExec, ServerState, ServerStateHandles, ServerStateLocks,
};
use std::sync::Arc;

// TODO macro the create locks fn
// TODO macro the unwrapping of state_handles

fn message_into_diagnostic(msg: Message) -> lsp_types::Diagnostic {
    // As ruff currently doesn't support the span of the error,
    // only have it span a single character
    let range = {
        // diagnostic is zero indexed, but message is 1-indexed
        let row_start = msg.location.row() as u32 - 1;
        let col_start = msg.location.column() as u32 - 1;
        let row_end = msg.end_location.row() as u32 - 1;
        let col_end = msg.end_location.column() as u32 - 1;
        let start = lsp_types::Position {
            line: row_start,
            character: col_start,
        };
        let end = lsp_types::Position {
            line: row_end,
            character: col_end,
        };
        lsp_types::Range { start, end }
    };
    let code = Some(lsp_types::NumberOrString::String(msg.kind.body()));
    let source = Some(String::from("ruff"));
    // uncertain if tui colours break things here
    let message = format!("{}", msg);
    lsp_types::Diagnostic {
        range,
        code,
        source,
        message,
        severity: Some(lsp_types::DiagnosticSeverity::WARNING),
        code_description: None,
        tags: None,
        related_information: None,
        data: None,
    }
}

// NOTE require interface from ruff before checks can be run
pub fn run_diagnostic_op(document_uri: lsp_types::Url) -> ServerNotification {
    let exec: ServerNotificationExec = Box::new(
        move |state_handles: ServerStateHandles<'_>, _scheduler_channel: Sender<ScheduledTask>| {
            Box::pin(async move {
                let open_buffers = match state_handles.open_buffers.unwrap() {
                    RwGuarded::Read(x) => x,
                    _ => unreachable!(),
                };
                let messages: Vec<Message> = {
                    if let Some(buffer) = open_buffers.get(&document_uri) {
                        let doc = buffer.iter().collect::<String>();
                        if let Ok(path) = document_uri.to_file_path() {
                            check(&path, &doc, true).unwrap_or_default()
                        } else {
                            vec![]
                        }
                    } else {
                        vec![]
                    }
                };
                let diagnostics = messages
                    .into_iter()
                    .map(message_into_diagnostic)
                    .collect::<Vec<_>>();
                RpcNotification::new(
                    "textDocument/publishDiagnostics".to_string(),
                    Some(
                        serde_json::to_value(lsp_types::PublishDiagnosticsParams {
                            uri: document_uri,
                            diagnostics,
                            version: None,
                        })
                        .unwrap(),
                    ),
                )
                .into()
            })
        },
    );
    let create_locks: CreateLocksFn = Box::new(|state: Arc<Mutex<ServerState>>| {
        Box::pin(async move {
            let handle = state.lock().await;
            let open_buffers = Some(RwReq::Read(handle.open_buffers.clone()));
            ServerStateLocks {
                open_buffers,
                ..Default::default()
            }
        })
    });
    ServerNotification { exec, create_locks }
}
