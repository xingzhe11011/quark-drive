use quark_core::CoreEvent;

#[test]
fn transfer_progress_event_uses_camel_case_fields() {
    let event = CoreEvent::TransferProgress {
        id: "transfer-1".to_string(),
        status: "running".to_string(),
        phase: "download".to_string(),
        done_bytes: 1024,
        total_bytes: 4096,
        speed_bps: 512.0,
    };
    let value = serde_json::to_value(event).unwrap();
    assert_eq!(value["type"], "transferProgress");
    assert_eq!(value["doneBytes"], 1024);
    assert_eq!(value["totalBytes"], 4096);
    assert_eq!(value["speedBps"], 512.0);
    assert!(value.get("done_bytes").is_none());
    assert!(value.get("total_bytes").is_none());
    assert!(value.get("speed_bps").is_none());
}
