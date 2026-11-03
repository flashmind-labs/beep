//! Short lines live mode says while a tool runs and the reply has been
//! quiet, such as "Let me check build-box". Picked from templates by the
//! kind of step, in the language the user spoke; no model call.

/// The languages the lines come in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    En,
    Es,
    Ca,
    Fr,
    De,
    It,
    Pt,
}

/// What a tool step is doing, from its label.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    /// A command on a machine, by name.
    Machine(String),
    Mail,
    Calendar,
    Web,
    Other,
}

/// Common short words of each language. The one with most hits in the
/// user's message wins; English when none does.
const WORDS: &[(Lang, &[&str])] = &[
    (
        Lang::En,
        &[
            "the", "is", "and", "my", "what", "to", "you", "it", "me", "on", "can", "check",
        ],
    ),
    (
        Lang::Es,
        &[
            "el", "la", "los", "las", "es", "qué", "que", "mi", "y", "por", "para", "cómo", "está",
            "hay", "puedes",
        ],
    ),
    (
        Lang::Ca,
        &[
            "el", "la", "els", "les", "és", "què", "que", "meu", "i", "per", "com", "està", "hi",
            "pots", "amb",
        ],
    ),
    (
        Lang::Fr,
        &[
            "le", "la", "les", "est", "et", "mon", "ma", "quoi", "que", "pour", "comment", "peux",
            "tu", "il", "sur",
        ],
    ),
    (
        Lang::De,
        &[
            "der", "die", "das", "ist", "und", "mein", "was", "wie", "kannst", "du", "auf",
            "nicht", "ich",
        ],
    ),
    (
        Lang::It,
        &[
            "il", "la", "gli", "è", "e", "mio", "cosa", "che", "per", "come", "puoi", "sono",
            "della",
        ],
    ),
    (
        Lang::Pt,
        &[
            "o", "a", "os", "as", "é", "e", "meu", "que", "para", "como", "pode", "está", "não",
            "você",
        ],
    ),
];

/// The language `said` is most likely in.
pub fn language(said: &str) -> Lang {
    let words: Vec<String> = said
        .split(|c: char| !c.is_alphanumeric() && c != '·' && c != '\'')
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let mut best = (Lang::En, 0);
    for (lang, common) in WORDS {
        let hits = words
            .iter()
            .filter(|w| common.contains(&w.as_str()))
            .count();
        if hits > best.1 {
            best = (*lang, hits);
        }
    }
    best.0
}

fn kind(step: &str) -> Kind {
    let lower = step.to_lowercase();
    let command = lower.starts_with("running ") || lower.starts_with("ran ");
    if command
        && let Some((_, host)) = step.rsplit_once(" on ")
        && let Some(host) = host.split_whitespace().next()
    {
        return Kind::Machine(host.trim_end_matches(['.', ',']).to_owned());
    }
    if ["mail", "inbox", "fastmail", "gmail", "outlook"]
        .iter()
        .any(|w| lower.contains(w))
    {
        Kind::Mail
    } else if ["calendar", "event", "meeting"]
        .iter()
        .any(|w| lower.contains(w))
    {
        Kind::Calendar
    } else if ["search", "web", "browse", "page", "reading http"]
        .iter()
        .any(|w| lower.contains(w))
    {
        Kind::Web
    } else {
        Kind::Other
    }
}

/// The line to say for tool step `step`, in the language of `said`.
/// `still` gives the line for a long wait instead.
pub fn line(step: &str, said: &str, still: bool) -> String {
    let lang = language(said);
    if still {
        return match lang {
            Lang::En => "Still on it.",
            Lang::Es => "Sigo con ello.",
            Lang::Ca => "Encara hi soc.",
            Lang::Fr => "J'y suis encore.",
            Lang::De => "Bin noch dran.",
            Lang::It => "Ci sto ancora lavorando.",
            Lang::Pt => "Ainda estou nisso.",
        }
        .to_owned();
    }
    match kind(step) {
        Kind::Machine(host) => match lang {
            Lang::En => format!("Let me check {host}."),
            Lang::Es => format!("Voy a mirar {host}."),
            Lang::Ca => format!("Miro {host}."),
            Lang::Fr => format!("Je regarde {host}."),
            Lang::De => format!("Ich schaue auf {host} nach."),
            Lang::It => format!("Controllo {host}."),
            Lang::Pt => format!("Vou verificar {host}."),
        },
        kind => match (kind, lang) {
            (Kind::Mail, Lang::En) => "Looking at your inbox.",
            (Kind::Mail, Lang::Es) => "Miro tu correo.",
            (Kind::Mail, Lang::Ca) => "Miro el teu correu.",
            (Kind::Mail, Lang::Fr) => "Je regarde tes mails.",
            (Kind::Mail, Lang::De) => "Ich schaue in deine Mails.",
            (Kind::Mail, Lang::It) => "Guardo la tua posta.",
            (Kind::Mail, Lang::Pt) => "Vou ver o teu email.",
            (Kind::Calendar, Lang::En) => "Checking your calendar.",
            (Kind::Calendar, Lang::Es) => "Miro tu calendario.",
            (Kind::Calendar, Lang::Ca) => "Miro el teu calendari.",
            (Kind::Calendar, Lang::Fr) => "Je regarde ton agenda.",
            (Kind::Calendar, Lang::De) => "Ich schaue in deinen Kalender.",
            (Kind::Calendar, Lang::It) => "Guardo il tuo calendario.",
            (Kind::Calendar, Lang::Pt) => "Vou ver o teu calendário.",
            (Kind::Web, Lang::En) => "Let me look that up.",
            (Kind::Web, Lang::Es) => "Lo busco.",
            (Kind::Web, Lang::Ca) => "Ho busco.",
            (Kind::Web, Lang::Fr) => "Je cherche.",
            (Kind::Web, Lang::De) => "Ich schaue nach.",
            (Kind::Web, Lang::It) => "Lo cerco.",
            (Kind::Web, Lang::Pt) => "Vou procurar.",
            (_, Lang::En) => "One moment.",
            (_, Lang::Es) => "Un momento.",
            (_, Lang::Ca) => "Un moment.",
            (_, Lang::Fr) => "Un instant.",
            (_, Lang::De) => "Einen Moment.",
            (_, Lang::It) => "Un attimo.",
            (_, Lang::Pt) => "Um momento.",
        }
        .to_owned(),
    }
}
