use super::*;
use std::time::UNIX_EPOCH;

fn listed(service: Service) -> Credential {
    Credential {
        id: "key.1.openrouter".into(),
        service,
        last4: "a1b2".into(),
        account: None,
        env_var: "OPENROUTER_API_KEY".into(),
        to_machines: true,
        created_at: UNIX_EPOCH,
        unreadable: false,
    }
}

/// A listed credential as released apps wrote it, `service` being what
/// `wire_enum!` made of `CredentialKind`: `[op]` or `[op, text]`.
fn released(service: impl serde::Serialize) -> Vec<u8> {
    let fields = (
        "key.1.openrouter",
        service,
        "a1b2",
        None::<String>,
        "OPENROUTER_API_KEY",
        true,
        UNIX_EPOCH,
        false,
    );
    rmp_serde::to_vec(&fields).unwrap()
}

#[test]
fn a_service_reads_as_released_apps_wrote_it() {
    let cases = [
        (released((0u8,)), Service::GitHub),
        (
            released((1u8, "https://git.example.com")),
            Service::Forgejo {
                url: "https://git.example.com".parse().unwrap(),
            },
        ),
        (
            released((2u8, "openrouter")),
            Service::ApiKey {
                provider: ApiKey::OpenRouter,
            },
        ),
    ];

    for (bytes, service) in cases {
        let read: Credential = crate::lenient::from_slice(&bytes).unwrap();
        assert_eq!(read, listed(service.clone()));
        assert_eq!(rmp_serde::to_vec(&listed(service)).unwrap(), bytes);
    }
}

#[test]
fn the_apps_read_a_service_by_its_kind() {
    let cases = [
        (Service::GitHub, r#"{"kind":"github"}"#),
        (
            Service::Forgejo {
                url: "https://git.example.com/".parse().unwrap(),
            },
            r#"{"kind":"forgejo","url":"https://git.example.com"}"#,
        ),
        (
            Service::ApiKey {
                provider: ApiKey::ElevenLabs,
            },
            r#"{"kind":"api_key","provider":"elevenlabs"}"#,
        ),
    ];

    for (service, json) in cases {
        assert_eq!(serde_json::to_string(&service).unwrap(), json);
        assert_eq!(serde_json::from_str::<Service>(json).unwrap(), service);
    }
}
