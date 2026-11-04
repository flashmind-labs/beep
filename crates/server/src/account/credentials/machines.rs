//! The shared teacli check, run against a fake shell.

use crate::protocol::CredentialCheck;
use beep_projects::{
    credential_check::machines::{Reach, check_from},
    model::ForgejoUrl,
};
use std::sync::Arc;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::testing::FakeShell;

    fn url() -> ForgejoUrl {
        "http://forge.example.lan:3000".parse().unwrap()
    }

    fn reach(name: &str, shell: &Arc<FakeShell>) -> Reach {
        Reach {
            name: name.to_owned(),
            shell: shell.clone(),
        }
    }

    #[tokio::test]
    async fn a_machine_that_reaches_the_server_checks_the_token() {
        let off = Arc::new(FakeShell::default());
        off.answer(
            "teacli",
            1,
            r#"{"error":true,"code":1,"message":"dial tcp: connection refused"}"#,
        );
        let on = Arc::new(FakeShell::default());
        on.answer("teacli", 0, r#"{"id":1,"login":"octo","full_name":"Octo"}"#);
        let checked = check_from(&[reach("mac-box", &off), reach("build-box", &on)], &url()).await;
        assert_eq!(
            checked,
            CredentialCheck::Valid {
                account: "octo".into(),
                machine: Some("build-box".into()),
            }
        );
        assert_eq!(
            on.ran(),
            ["teacli whoami --host http://forge.example.lan:3000 --format json"]
        );
    }
}
