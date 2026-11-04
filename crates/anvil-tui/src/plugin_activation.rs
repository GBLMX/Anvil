//! Plugin activation event parsing and matching.

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ActivationEvent {
    Startup,
    Command(String),
    Language(String),
    WorkspaceContains(String),
}

pub(crate) fn parse_event(value: &str) -> Option<ActivationEvent> {
    if value == "onStartup" {
        return Some(ActivationEvent::Startup);
    }
    for (prefix, constructor) in [
        (
            "onCommand:",
            ActivationEvent::Command as fn(String) -> ActivationEvent,
        ),
        ("onLanguage:", ActivationEvent::Language),
        ("onWorkspaceContains:", ActivationEvent::WorkspaceContains),
    ] {
        if let Some(value) = value.strip_prefix(prefix)
            && !value.is_empty()
        {
            return Some(constructor(value.to_owned()));
        }
    }
    None
}

pub(crate) fn matches(events: &[String], event: &ActivationEvent) -> bool {
    events
        .iter()
        .filter_map(|value| parse_event(value))
        .any(|candidate| candidate == *event)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_supported_events() {
        assert_eq!(parse_event("onStartup"), Some(ActivationEvent::Startup));
        assert_eq!(
            parse_event("onCommand:uuid.generate"),
            Some(ActivationEvent::Command("uuid.generate".to_owned()))
        );
        assert_eq!(
            parse_event("onLanguage:rust"),
            Some(ActivationEvent::Language("rust".to_owned()))
        );
        assert_eq!(
            parse_event("onWorkspaceContains:Cargo.toml"),
            Some(ActivationEvent::WorkspaceContains("Cargo.toml".to_owned()))
        );
        assert_eq!(parse_event("onCommand:"), None);
        assert_eq!(parse_event("onUnknown"), None);
    }

    #[test]
    fn matches_exact_events_only() {
        let events = vec![
            "onStartup".to_owned(),
            "onLanguage:rust".to_owned(),
            "onCommand:demo.run".to_owned(),
        ];
        assert!(matches(&events, &ActivationEvent::Startup));
        assert!(matches(
            &events,
            &ActivationEvent::Language("rust".to_owned())
        ));
        assert!(matches(
            &events,
            &ActivationEvent::Command("demo.run".to_owned())
        ));
        assert!(!matches(
            &events,
            &ActivationEvent::Language("go".to_owned())
        ));
        assert!(!matches(
            &events,
            &ActivationEvent::WorkspaceContains("Cargo.toml".to_owned())
        ));
    }
}
