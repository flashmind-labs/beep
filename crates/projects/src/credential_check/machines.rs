//! Checks a Forgejo token with `teacli whoami` from the user's machines, for servers only they reach.

use crate::{Shell, ShellOutput, commands::parse_login, model::ForgejoUrl, quote};
use beep_proto::CredentialCheck;
use futures::future::join_all;
use std::{sync::Arc, time::Duration};

/// The most machines asked at once.
pub const MAX_MACHINES: usize = 4;
/// How long one machine's check may take.
const LIMIT: Duration = Duration::from_secs(20);

/// One of the user's online machines with teacli.
pub struct Reach {
    pub name: String,
    pub shell: Arc<dyn Shell>,
}

/// What one machine's teacli said.
#[derive(Debug, PartialEq, Eq)]
enum Said {
    Valid(String),
    Refused,
    Unreachable,
    /// Its teacli does not know the server, or it has no teacli.
    NotSetUp,
}

/// Reads `teacli whoami` output: exit 4 is refused, 2 is unknown server, 1 may name the status.
fn read(ran: &ShellOutput) -> Said {
    let text = ran.output.to_ascii_lowercase();
    match ran.exit_code {
        Some(0) => match parse_login(&ran.output) {
            Ok(login) => Said::Valid(login),
            Err(_) => Said::Unreachable,
        },
        Some(4) => Said::Refused,
        Some(2 | 126 | 127) => Said::NotSetUp,
        _ if text.contains("command not found") || text.contains("unknown host") => Said::NotSetUp,
        _ if ["401", "403", "unauthorized", "forbidden", "invalid token"]
            .iter()
            .any(|word| text.contains(word)) =>
        {
            Said::Refused
        }
        _ => Said::Unreachable,
    }
}

/// Checks the token for `url` from up to [`MAX_MACHINES`] machines at once.
pub async fn check_from(machines: &[Reach], url: &ForgejoUrl) -> CredentialCheck {
    let machines = &machines[..machines.len().min(MAX_MACHINES)];
    let command = format!("teacli whoami --host {} --format json", quote(url.as_str()));
    let said = join_all(machines.iter().map(|machine| async {
        match machine.shell.run(&command, LIMIT).await {
            Ok(ran) => read(&ran),
            Err(e) => {
                log::info!(
                    "credentials: {} could not check a Forgejo token: {e:#}",
                    machine.name
                );
                Said::Unreachable
            }
        }
    }))
    .await;
    let host = url.host();
    let named = || machines.iter().zip(&said);
    if let Some((machine, Said::Valid(login))) =
        named().find(|(_, said)| matches!(said, Said::Valid(_)))
    {
        return CredentialCheck::Valid {
            account: login.clone(),
            machine: Some(machine.name.clone()),
        };
    }
    if let Some((machine, _)) = named().find(|(_, said)| **said == Said::Refused) {
        return CredentialCheck::Refused {
            machine: Some(machine.name.clone()),
            message: format!(
                "{host} refused the token from {}. Check that it is current and has access.",
                machine.name
            ),
        };
    }
    let tried: Vec<&str> = named()
        .filter(|(_, said)| **said == Said::Unreachable)
        .map(|(machine, _)| machine.name.as_str())
        .collect();
    let message = if tried.is_empty() {
        format!(
            "{host} is not reachable from the server, and none of your online machines has \
             teacli set up for it."
        )
    } else {
        format!(
            "{host} is not reachable from the server or your machines (tried {}).",
            tried.join(", ")
        )
    };
    CredentialCheck::Unreachable { message }
}
