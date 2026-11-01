//! The models every user can pick for their own turns. Whoever has not
//! picked one runs on the server default. Also keeps who is an admin, for
//! the operator features that stay theirs (machines, account events,
//! watchers).

use crate::{
    protocol::{ModelChoice, Models},
    research::ResearchConfig,
};
use flashmind::types::{LlmProvider, Model, ModelInfo, ProviderPreferences, ReasoningLevel};
use iroh::EndpointId;
use rust_decimal::{Decimal, prelude::ToPrimitive};
use std::{
    collections::{HashMap, HashSet},
    iter,
    sync::{PoisonError, RwLock},
};

pub struct ModelChoices {
    /// The keys the config names as admins.
    admin_keys: HashSet<String>,
    /// The accounts those keys sign into, filled in as they are seen.
    admins: RwLock<HashSet<String>>,
    /// The server default first, then the configured choices.
    choices: Vec<Choice>,
    /// Provider routing for models that do not use the server's own.
    routing: HashMap<Model, ProviderPreferences>,
    /// The offered models Jev may pick per prompt, in choice order.
    auto: Vec<AutoModel>,
    /// The models research roles run on; `None` turns research off.
    research: Option<ResearchConfig>,
}

/// A model Jev may pick, with what it is good for.
pub struct AutoModel {
    pub model: Model,
    pub name: String,
    pub when: String,
}

struct Choice {
    model: Model,
    name: String,
    input_price: Option<f64>,
    output_price: Option<f64>,
    sees_images: bool,
}

impl ModelChoices {
    /// Only the default, with no admins.
    pub fn only(default: Model) -> Self {
        Self {
            admin_keys: HashSet::new(),
            admins: RwLock::default(),
            choices: vec![Choice {
                name: default.name().to_owned(),
                model: default,
                input_price: None,
                output_price: None,
                sees_images: false,
            }],
            routing: HashMap::new(),
            auto: Vec::new(),
            research: None,
        }
    }

    /// Looks up names, prices and vision in the provider's model list. A
    /// choice the provider does not list is left out with a warning. Without
    /// a list every choice stays, unpriced.
    pub async fn resolve(
        provider: &dyn LlmProvider,
        default: Model,
        configured: Vec<Model>,
        admins: &[EndpointId],
    ) -> Self {
        let listed = provider.list_models().await;
        if listed.is_none() && !configured.is_empty() {
            log::warn!("the provider lists no models; model choices are unpriced");
        }
        let mut choices = Vec::new();
        for (model, is_default) in
            iter::once((default, true)).chain(configured.into_iter().map(|model| (model, false)))
        {
            if choices.iter().any(|c: &Choice| c.model == model) {
                continue;
            }
            let info = listed
                .as_deref()
                .and_then(|list| list.iter().find(|info| info.id == model.name()));
            let choice = match info {
                Some(info) => Choice::listed(model, info),
                None if listed.is_some() && !is_default => {
                    log::warn!("model choice not offered by the provider; model={model}");
                    continue;
                }
                None => Choice {
                    name: model.name().to_owned(),
                    sees_images: provider.capabilities(&model).await.images,
                    model,
                    input_price: None,
                    output_price: None,
                },
            };
            choices.push(choice);
        }
        Self {
            admin_keys: admins.iter().map(ToString::to_string).collect(),
            admins: RwLock::default(),
            choices,
            routing: HashMap::new(),
            auto: Vec::new(),
            research: None,
        }
    }

    /// Routes these models with their own provider preferences.
    pub fn with_routing(mut self, routing: HashMap<Model, ProviderPreferences>) -> Self {
        self.routing = routing;
        self
    }

    /// Lets Jev pick among these models, each with what it is good for. A
    /// model that is not offered is left out with a warning.
    pub fn with_auto(mut self, mut described: HashMap<Model, String>) -> Self {
        self.auto = self
            .choices
            .iter()
            .filter_map(|choice| {
                described.remove(&choice.model).map(|when| AutoModel {
                    model: choice.model.clone(),
                    name: choice.name.clone(),
                    when,
                })
            })
            .collect();
        for model in described.keys() {
            log::warn!("auto model is not a model choice; model={model}");
        }
        self
    }

    /// Turns research on with these role models. Each must be offered: a
    /// role whose model is not turns research off with a warning, and a
    /// scout choice that is not is left out.
    pub fn with_research(mut self, configured: Option<ResearchConfig>) -> Self {
        let Some(mut config) = configured else {
            return self;
        };
        let roles = [
            ("lead", &config.lead),
            ("scout", &config.scout),
            ("critic", &config.critic),
            ("writer", &config.writer),
        ];
        for (role, model) in roles {
            if self.find(&model.model.to_string()).is_none() {
                log::warn!(
                    "research model is not a model choice; research is off; role={role} model={}",
                    model.model
                );
                return self;
            }
        }
        config.scout_choices.retain(|model| {
            let offered = self.find(&model.to_string()).is_some();
            if !offered {
                log::warn!("research scout choice is not a model choice; model={model}");
            }
            offered
        });
        self.research = Some(config);
        self
    }

    /// The research roles' models, when research is on.
    pub fn research(&self) -> Option<&ResearchConfig> {
        self.research.as_ref()
    }

    /// The models Jev may pick, empty when Auto is unavailable.
    pub fn auto(&self) -> &[AutoModel] {
        &self.auto
    }

    /// The display name of an offered model.
    pub fn name_of(&self, model: &Model) -> Option<&str> {
        self.choices
            .iter()
            .find(|choice| &choice.model == model)
            .map(|choice| choice.name.as_str())
    }

    /// The provider routing for `model`, when it has its own.
    pub fn routing(&self, model: &Model) -> Option<&ProviderPreferences> {
        self.routing.get(model)
    }

    pub fn is_admin(&self, user: &str) -> bool {
        self.admins
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(user)
    }

    /// Every account admitted as an admin so far.
    pub fn admins(&self) -> Vec<String> {
        self.admins
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect()
    }

    /// Makes account `user` an admin when `key`, one of its keys, is an
    /// admin key in the config.
    pub fn admit(&self, user: &str, key: &str) {
        if self.admin_keys.contains(key) {
            self.admins
                .write()
                .unwrap_or_else(PoisonError::into_inner)
                .insert(user.to_owned());
        }
    }

    /// Makes account `user` an admin, as the owner of a single-user server.
    pub fn make_admin(&self, user: &str) {
        self.admins
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(user.to_owned());
    }

    pub fn default_model(&self) -> &Model {
        &self.choices[0].model
    }

    /// The offered model with `id`.
    pub fn find(&self, id: &str) -> Option<&Model> {
        self.choices
            .iter()
            .map(|choice| &choice.model)
            .find(|model| model.to_string() == id)
    }

    /// The model a user's turns run on, from what they picked. A pick that
    /// is no longer offered falls back to the default.
    pub fn for_user(&self, picked: Option<&str>) -> &Model {
        picked
            .and_then(|id| self.find(id))
            .unwrap_or_else(|| self.default_model())
    }

    /// What a user sees in the picker, with their settings marked.
    pub fn for_picker(&self, picked: &Picked) -> Models {
        let choices = self
            .choices
            .iter()
            .enumerate()
            .map(|(i, choice)| ModelChoice {
                id: choice.model.to_string(),
                name: choice.name.clone(),
                input_price: choice.input_price,
                output_price: choice.output_price,
                sees_images: choice.sees_images,
                default: i == 0,
            })
            .collect();
        let auto_available = !self.auto.is_empty();
        Models {
            choices,
            selected: picked.model.to_string(),
            thinking: picked.thinking,
            auto: auto_available && picked.auto,
            auto_available,
        }
    }
}

/// A user's own model settings.
pub struct Picked<'a> {
    pub model: &'a Model,
    pub thinking: ReasoningLevel,
    pub auto: bool,
}

impl Choice {
    fn listed(model: Model, info: &ModelInfo) -> Self {
        Self {
            name: info
                .name
                .as_deref()
                .map_or_else(|| model.name().to_owned(), short_name),
            input_price: per_million(info.pricing.prompt),
            output_price: per_million(info.pricing.completion),
            sees_images: info.capabilities.images,
            model,
        }
    }
}

/// A listed name without its maker, which the provider puts first:
/// "OpenAI: GPT-5.6 Luna" reads "GPT-5.6 Luna".
fn short_name(name: &str) -> String {
    name.split_once(": ")
        .map_or(name, |(_, short)| short)
        .to_owned()
}

/// A per-token price as USD per million tokens.
fn per_million(price: Option<Decimal>) -> Option<f64> {
    (price? * Decimal::from(1_000_000)).to_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choices() -> ModelChoices {
        let mut models = ModelChoices::only("openrouter:z-ai/glm-5.3-flash".parse().unwrap());
        models.choices.push(Choice {
            model: "openrouter:openai/gpt-5.6-luna".parse().unwrap(),
            name: "GPT-5.6 Luna".into(),
            input_price: Some(0.1),
            output_price: Some(0.5),
            sees_images: true,
        });
        models
    }

    #[test]
    fn anyone_s_pick_counts() {
        let models = choices();
        let luna = Some("openrouter:openai/gpt-5.6-luna");
        assert_eq!(models.for_user(luna).name(), "openai/gpt-5.6-luna");
        assert_eq!(models.for_user(None).name(), "z-ai/glm-5.3-flash");
    }

    #[test]
    fn a_pick_no_longer_offered_falls_back_to_the_default() {
        let models = choices();
        let gone = Some("openrouter:retired/model");
        assert_eq!(models.for_user(gone).name(), "z-ai/glm-5.3-flash");
    }

    fn picked(model: &Model, auto: bool) -> Picked<'_> {
        Picked {
            model,
            thinking: ReasoningLevel::Low,
            auto,
        }
    }

    #[test]
    fn everyone_sees_choices() {
        let models = choices();
        let default = models.default_model().clone();
        let picked = picked(&default, false);
        let choices = models.for_picker(&picked).choices;
        assert_eq!(choices.len(), 2);
        assert!(choices[0].default);
        assert_eq!(choices[1].name, "GPT-5.6 Luna");
    }

    #[test]
    fn auto_takes_only_offered_models_in_choice_order() {
        let luna: Model = "openrouter:openai/gpt-5.6-luna".parse().unwrap();
        let flash: Model = "openrouter:z-ai/glm-5.3-flash".parse().unwrap();
        let gone: Model = "openrouter:retired/model".parse().unwrap();
        let models = choices().with_auto(HashMap::from([
            (luna.clone(), "Demanding work".to_owned()),
            (flash.clone(), "Quick lookups".to_owned()),
            (gone, "Anything".to_owned()),
        ]));
        let auto: Vec<_> = models.auto().iter().map(|a| &a.model).collect();
        assert_eq!(auto, [&flash, &luna]);
        assert_eq!(models.auto()[1].name, "GPT-5.6 Luna");

        let on = picked(&flash, true);
        let shown = models.for_picker(&on);
        assert!(shown.auto && shown.auto_available);
        let off = models.for_picker(&picked(&flash, false));
        assert!(!off.auto && off.auto_available);
        assert!(!choices().for_picker(&on).auto);
    }

    #[test]
    fn listed_names_drop_their_maker() {
        assert_eq!(short_name("OpenAI: GPT-5.6 Luna"), "GPT-5.6 Luna");
        assert_eq!(short_name("GPT-5.6 Luna"), "GPT-5.6 Luna");
    }

    #[test]
    fn prices_are_per_million_tokens() {
        let per_token: Decimal = "0.0000005".parse().unwrap();
        assert_eq!(per_million(Some(per_token)), Some(0.5));
    }
}
