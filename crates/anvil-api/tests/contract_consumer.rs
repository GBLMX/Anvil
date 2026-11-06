use anvil_api::{
    API_VERSION, ApplyEditRequest, CopyRequest, DocumentEdit, EventSubscribeRequest, ExecRequest,
    FetchRequest, METHODS, MethodType, NotificationSeverity, NotifyRequest, PathRequest,
    ReadFileRequest, RegisterCommandRequest, RenameRequest, SetModeRequest, StatusRequest,
    TextRange,
};

fn round_trip<T>(value: &T)
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let encoded = serde_json::to_vec(value).expect("encode");
    let decoded: T = serde_json::from_slice(&encoded).expect("decode");
    assert_eq!(&decoded, value);
}

#[test]
fn independent_consumer_covers_the_catalog() {
    assert_eq!(API_VERSION, "1.0.0");
    for spec in METHODS {
        assert!(
            !matches!(spec.params, MethodType::Empty) || spec.result != MethodType::Empty,
            "{} has an empty contract",
            spec.name
        );
    }
}

#[test]
fn independent_consumer_round_trips_requests() {
    round_trip(&SetModeRequest {
        mode: anvil_api::InteractionMode::Select,
    });
    round_trip(&ApplyEditRequest {
        buffer_id: 1,
        base_revision: 2,
        edits: vec![DocumentEdit {
            range: TextRange { start: 0, end: 1 },
            text: "x".to_owned(),
        }],
    });
    round_trip(&RegisterCommandRequest {
        id: "demo.run".to_owned(),
        title: "Run".to_owned(),
    });
    round_trip(&NotifyRequest {
        message: "saved".to_owned(),
        severity: NotificationSeverity::Info,
    });
    round_trip(&StatusRequest {
        message: "ready".to_owned(),
    });
    round_trip(&EventSubscribeRequest {
        events: vec!["document.changed".to_owned()],
    });
    round_trip(&PathRequest {
        path: "src".to_owned(),
    });
    round_trip(&ReadFileRequest {
        path: "README.md".to_owned(),
        offset: 0,
        max_bytes: 4096,
    });
    round_trip(&RenameRequest {
        from: "a".to_owned(),
        to: "b".to_owned(),
    });
    round_trip(&CopyRequest {
        from: "a".to_owned(),
        to: "b".to_owned(),
    });
    round_trip(&FetchRequest {
        url: "https://example.com".to_owned(),
        max_bytes: 4096,
        headers: Default::default(),
    });
    round_trip(&ExecRequest {
        tool: "true".to_owned(),
        args: Vec::new(),
        timeout_ms: 1000,
    });
}
