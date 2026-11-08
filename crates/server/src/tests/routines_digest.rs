use super::*;

fn email(mailbox: Mailbox, hours_ago: i64, text: &str, flagged: bool) -> DigestEmail {
    DigestEmail {
        mailbox,
        received: Utc::now() - Days::hours(hours_ago),
        text: text.to_owned(),
        flagged,
    }
}

#[test]
fn trimming_keeps_flagged_emails_and_the_newest_others() {
    let mut report = Report::default();
    report
        .emails
        .push(email(Mailbox::Gmail, 30, "old flagged", true));
    for hours in 0..MAX_EMAILS as i64 + 5 {
        report
            .emails
            .push(email(Mailbox::Gmail, hours, &format!("{hours}"), false));
    }
    report.trim();
    assert_eq!(report.emails.len(), MAX_EMAILS);
    assert!(
        report
            .emails
            .iter()
            .any(|email| email.text == "old flagged")
    );
    assert_eq!(report.emails[0].text, "0");
    assert!(!report.emails.iter().any(|email| email.text == "39"));
}
