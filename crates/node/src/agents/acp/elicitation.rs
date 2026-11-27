//! Form elicitations: what an agent asks through `elicitation/create`, such
//! as Claude Code's AskUserQuestion or an MCP server's form, put to the user
//! one field at a time and sent back once every field has its answer.

use agent_client_protocol::{
    Responder,
    v1::schema::{
        CreateElicitationRequest, CreateElicitationResponse, ElicitationAcceptAction,
        ElicitationAction, ElicitationContentValue, ElicitationMode, ElicitationPropertySchema,
        EnumOption, MultiSelectItems,
    },
};
use beep_proto::node::agent::{AnswerOption, OptionKind, Question};
use std::collections::{BTreeMap, VecDeque};

/// The option that skips the whole form.
const SKIP: &str = "skip";

/// The suffixes of a choice's companion field for the user's own words, as
/// the Claude adapter (`question_0_custom`), the Codex adapter
/// (`question_0_note`) and Antigravity (`q0_other`) name it.
const CUSTOM: [&str; 3] = ["_custom", "_note", "_other"];

/// A form waiting for the user, its fields still to answer first in line.
pub(in crate::agents) struct Form {
    responder: Responder<CreateElicitationResponse>,
    fields: VecDeque<Field>,
    content: BTreeMap<String, ElicitationContentValue>,
}

/// One field of a form, as the question it is put to the user as.
struct Field {
    key: String,
    /// The field that takes the user's own words instead of a pick.
    custom: Option<String>,
    value: Value,
    question: Question,
}

/// What a field's answer is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Value {
    /// One of its options, or words of the user's own.
    Pick,
    /// Any of its options, several at once.
    Picks,
    Text,
    Boolean,
    Integer,
    Number,
}

/// What became of a form after an answer.
pub(in crate::agents) enum Answer {
    /// More fields wait: the form asks its next question.
    Next(Box<Form>),
    /// The form was sent back, with the answer to show.
    Sent(String),
}

/// The fields of a form the user can fill in, first asked first.
pub(in crate::agents) struct Fields(VecDeque<Field>);

/// Reads the form an `elicitation/create` asks for; an error when it is no
/// form the user can fill in.
impl TryFrom<CreateElicitationRequest> for Fields {
    type Error = agent_client_protocol::Error;

    fn try_from(request: CreateElicitationRequest) -> Result<Self, Self::Error> {
        let ElicitationMode::Form(form) = request.mode else {
            return Err(agent_client_protocol::Error::invalid_params());
        };
        let codex_meta = request.meta.as_ref().and_then(|meta| meta.get("codex"));
        let codex = codex_meta.is_some();
        let background = codex_meta
            .and_then(|meta| meta.get("delivery"))
            .and_then(|delivery| delivery.as_str())
            == Some("async");
        let properties = form.requested_schema.properties;
        let single = properties
            .keys()
            .filter(|key| !is_custom(&properties, key))
            .count()
            == 1;
        let message = (!background && (single || codex)).then_some(request.message.as_str());

        let fields: VecDeque<Field> = properties
            .iter()
            .filter(|(key, _)| !is_custom(&properties, key))
            .filter_map(|(key, schema)| {
                let custom = CUSTOM
                    .iter()
                    .map(|suffix| format!("{key}{suffix}"))
                    .find(|custom| properties.contains_key(custom));
                field(key, schema, custom, message, codex)
            })
            .collect();
        if fields.is_empty() {
            return Err(agent_client_protocol::Error::invalid_params());
        }

        Ok(Self(fields))
    }
}

impl Form {
    pub(in crate::agents) fn new(
        Fields(fields): Fields,
        responder: Responder<CreateElicitationResponse>,
    ) -> Self {
        Self {
            responder,
            fields,
            content: BTreeMap::new(),
        }
    }

    /// The question of the field answered next.
    pub(in crate::agents) fn question(&self) -> &Question {
        // A form is never kept without a field to answer.
        &self.fields[0].question
    }

    /// Takes `reply` as the answer to the first field: one of its options by
    /// id, number or name, several for a multiple choice, or the user's own
    /// words. Skip declines the whole form.
    pub(in crate::agents) fn answer(
        mut self,
        reply: &str,
        choose: impl Fn(&[AnswerOption], &str) -> Option<AnswerOption>,
    ) -> Result<Answer, agent_client_protocol::Error> {
        let Some(field) = self.fields.pop_front() else {
            self.responder.respond(response(ElicitationAction::Cancel))?;
            return Ok(Answer::Sent("cancelled".into()));
        };
        let options = &field.question.options;
        let picked = choose(options, reply);

        if picked.as_ref().is_some_and(|option| option.id == SKIP) {
            self.responder.respond(response(ElicitationAction::Decline))?;
            return Ok(Answer::Sent("skipped".into()));
        }

        let said = field.answer(reply, picked, &choose, &mut self.content);

        if self.fields.is_empty() {
            let accept = ElicitationAcceptAction::new().content(self.content);
            self.responder
                .respond(response(ElicitationAction::Accept(accept)))?;
            return Ok(Answer::Sent(said));
        }

        Ok(Answer::Next(Box::new(self)))
    }

    /// Answers that the user never filled it in.
    pub(in crate::agents) fn cancel(self) -> Result<(), agent_client_protocol::Error> {
        self.responder.respond(response(ElicitationAction::Cancel))
    }
}

impl Field {
    /// Puts the answer to this field in `content`, and says what it was.
    fn answer(
        &self,
        reply: &str,
        picked: Option<AnswerOption>,
        choose: &impl Fn(&[AnswerOption], &str) -> Option<AnswerOption>,
        content: &mut BTreeMap<String, ElicitationContentValue>,
    ) -> String {
        let reply = reply.trim();
        let mut put = |key: &str, value: ElicitationContentValue| {
            content.insert(key.to_owned(), value);
        };

        match (self.value, picked) {
            (Value::Boolean, Some(option)) => {
                put(&self.key, ElicitationContentValue::Boolean(option.id == "true"));
                option.name
            }
            (Value::Pick, Some(option)) => {
                put(&self.key, option.id.as_str().into());
                option.name
            }
            (Value::Picks, _) => {
                let mut picks = Vec::new();
                let mut words = Vec::new();
                for part in reply.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                    match choose(&self.question.options, part) {
                        Some(option) if option.id != SKIP => picks.push(option),
                        _ => words.push(part),
                    }
                }
                let mut said: Vec<String> = picks.iter().map(|o| o.name.clone()).collect();
                put(
                    &self.key,
                    ElicitationContentValue::StringArray(
                        picks.into_iter().map(|option| option.id).collect(),
                    ),
                );
                // Words that name no option are the user's own answer.
                if let Some(custom) = self.custom.as_deref()
                    && !words.is_empty()
                {
                    let words = words.join(", ");
                    said.push(words.clone());
                    put(custom, words.into());
                }
                said.join(", ")
            }
            (Value::Pick | Value::Boolean, None) => {
                if let Some(custom) = &self.custom {
                    put(custom, reply.into());
                } else if self.value == Value::Pick {
                    put(&self.key, reply.into());
                }
                reply.to_owned()
            }
            (Value::Text, _) => {
                put(&self.key, reply.into());
                reply.to_owned()
            }
            (Value::Integer, _) => {
                if let Ok(n) = reply.parse::<i64>() {
                    put(&self.key, n.into());
                }
                reply.to_owned()
            }
            (Value::Number, _) => {
                if let Ok(n) = reply.parse::<f64>() {
                    put(&self.key, n.into());
                }
                reply.to_owned()
            }
        }
    }
}

/// The field `key` of a form as a question.
fn field(
    key: &str,
    schema: &ElicitationPropertySchema,
    custom: Option<String>,
    message: Option<&str>,
    codex: bool,
) -> Option<Field> {
    let titled = |options: &[EnumOption]| {
        options
            .iter()
            .map(|option| choice(&option.value, &option.title))
            .collect::<Vec<_>>()
    };
    let plain = |values: &[String]| values.iter().map(|v| choice(v, v)).collect::<Vec<_>>();

    let (value, title, description, options) = match schema {
        ElicitationPropertySchema::String(s) => {
            let options = match (&s.one_of, &s.enum_values) {
                (Some(options), _) => titled(options),
                (None, Some(values)) => plain(values),
                (None, None) => Vec::new(),
            };
            let value = if options.is_empty() {
                Value::Text
            } else {
                Value::Pick
            };
            (value, &s.title, &s.description, options)
        }
        ElicitationPropertySchema::Array(s) => {
            let options = match &s.items {
                MultiSelectItems::String(items) => plain(&items.values),
                MultiSelectItems::Titled(items) => titled(&items.options),
                _ => return None,
            };
            (Value::Picks, &s.title, &s.description, options)
        }
        ElicitationPropertySchema::Boolean(s) => (
            Value::Boolean,
            &s.title,
            &s.description,
            vec![choice("true", "Yes"), choice("false", "No")],
        ),
        ElicitationPropertySchema::Integer(s) => {
            (Value::Integer, &s.title, &s.description, Vec::new())
        }
        ElicitationPropertySchema::Number(s) => {
            (Value::Number, &s.title, &s.description, Vec::new())
        }
        _ => return None,
    };

    let title = title.as_deref();
    let description = description.as_deref();
    // Codex puts the question in the title and the form's status in the message.
    let text = if codex {
        title.or(description).or(message)
    } else {
        description.or(message).or(title)
    }
    .unwrap_or(key);
    let details = [
        codex.then_some(description).flatten(),
        codex.then_some(message).flatten(),
        (value == Value::Picks).then_some("Pick several by their numbers, such as 1, 3"),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.trim().is_empty() && *part != text)
    .collect::<Vec<_>>();
    let detail = (!details.is_empty()).then(|| details.join("\n"));
    let mut options = options;
    options.push(AnswerOption {
        id: SKIP.into(),
        name: "Skip".into(),
        kind: OptionKind::RejectOnce,
    });

    Some(Field {
        key: key.to_owned(),
        custom,
        value,
        question: Question {
            id: 0,
            text: crate::text::cut(text, beep_proto::node::agent::MAX_QUESTION_LINE),
            detail,
            options,
        },
    })
}

fn choice(id: &str, name: &str) -> AnswerOption {
    AnswerOption {
        id: id.to_owned(),
        name: name.to_owned(),
        kind: OptionKind::AllowOnce,
    }
}

/// Whether `key` is the companion of a choice, for the user's own words.
fn is_custom(properties: &BTreeMap<String, ElicitationPropertySchema>, key: &str) -> bool {
    CUSTOM.iter().any(|suffix| {
        key.strip_suffix(suffix)
            .is_some_and(|choice| properties.contains_key(choice))
    })
}

fn response(action: ElicitationAction) -> CreateElicitationResponse {
    CreateElicitationResponse::new(action)
}
