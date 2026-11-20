use super::*;
use crate::ShortTicket;
use iroh::SecretKey;
use iroh_tickets::Ticket;

fn ticket() -> TeamTicket {
    TeamTicket {
        inviter: SecretKey::generate().public(),
        repo: "https://GitHub.com/Org/Repo.git".parse().expect("repo"),
        secret: JoinSecret::generate(),
    }
}

#[test]
fn ticket_round_trips_in_capitals() {
    let ticket = ticket();
    let text = ticket.encode_string().to_ascii_uppercase();
    let back = TeamTicket::parse(&text).expect("reads capitals");

    assert_eq!(back.inviter, ticket.inviter);
    assert_eq!(back.repo, ticket.repo);
    assert!(back.secret.verify(&ticket.secret));
    assert!(text.to_ascii_lowercase().starts_with("beepteam1"));
}

#[test]
fn beepjoin3_is_not_a_team_ticket() {
    let short = ShortTicket {
        desktop: SecretKey::generate().public(),
        secret: JoinSecret::generate(),
    };

    assert_eq!(
        TeamTicket::parse(&short.encode_string()).unwrap_err(),
        TicketError::Unreadable
    );
}

#[test]
fn secret_hash_follows_the_secret() {
    let a = JoinSecret::generate();

    assert_eq!(secret_hash(&a), secret_hash(&a.clone()));
    assert_ne!(secret_hash(&a), secret_hash(&JoinSecret::generate()));
}
