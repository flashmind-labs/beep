use super::*;
use beep_proto::CreditScope;

fn credit(unit: CreditUnit, remaining: Option<&str>) -> ProviderCredits {
    ProviderCredits {
        provider: "openrouter".into(),
        scope: CreditScope::Account,
        unit,
        remaining: remaining.map(Into::into),
        total: Some("1000".into()),
        used: Some("377.6781329".into()),
        resets: None,
    }
}

#[test]
fn a_balance_reads_whole_dollars_in_the_bar_and_cents_in_the_popover() {
    let row = CreditRow::from(credit(CreditUnit::Usd, Some("622.3218671")));
    assert_eq!(row.remaining_short.as_deref(), Some("$622"));
    assert_eq!(row.remaining_full.as_deref(), Some("$622.32"));
    assert_eq!(row.used_full.as_deref(), Some("$377.68"));
    assert_eq!(row.total_full.as_deref(), Some("$1000.00"));
    assert_eq!(row.credit.remaining.as_deref(), Some("622.3218671"));
}

#[test]
fn halves_round_up() {
    let label = |text: &str, places| {
        Amount::parse(CreditUnit::Usd, text)
            .map(|a| a.label(places))
            .unwrap_or_default()
    };
    assert_eq!(label("622.5", 0), "$623");
    assert_eq!(label("0.125", 2), "$0.13");
    assert_eq!(label("-2.5", 0), "-$3");
    assert_eq!(label("-0.004", 2), "$0.00");
    assert_eq!(label("1e3", 0), "$1000");
}

#[test]
fn unknown_and_malformed_balances_have_no_label() {
    assert_eq!(
        CreditRow::from(credit(CreditUnit::Usd, None)).remaining_short,
        None
    );
    assert_eq!(
        CreditRow::from(credit(CreditUnit::Usd, Some("lots"))).remaining_full,
        None
    );
    assert_eq!(
        CreditRow::from(credit(CreditUnit::Credits, Some("12.5"))).remaining_short,
        Some("13".into())
    );
}
