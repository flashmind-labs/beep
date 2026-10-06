//! The models admins can pick for their own turns. Everyone else, and every
//! admin who has not picked one, runs on the server default.

use crate::protocol::{ModelChoice, Models};
use flashmind::types::{LlmProvider, Model, ModelInfo, ProviderPreferences, ReasoningLevel};
use iroh::EndpointId;
use rust_decimal::{Decimal, prelude::ToPrimitive};
use std::{
    collections::{HashMap, HashSet},
    iter,
};

/// The most a pickable model may cost, in USD per million output tokens.
pub const MAX_OUTPUT_PRICE: f64 = 4.0;

pub struct ModelChoices {
    admins: HashSet<String>,
    /// The server default first, then the configured choices.
    choices: Vec<Choice>,
    /// Provider routing for models that do not use the server's own.
    routing: HashMap<Model, ProviderPreferences>,
    /// The offered models Jev may pick per prompt, in choice order.
    auto: Vec<AutoModel>,
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
            admins: HashSet::new(),
            choices: vec![Choice {
                name: default.name().to_owned(),
                model: default,
                input_price: None,
                output_price: None,
                sees_images: false,
            }],
            routing: HashMap::new(),
            auto: Vec::new(),
        }
    }

    /// Looks up names, prices and vision in the provider's model list. A
    /// choice the provider does not list, or that costs more than
    /// `MAX_OUTPUT_PRICE`, is left out with a warning. Without a list every
    /// choice stays, unpriced.
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
            if !is_default && choice.output_price.is_some_and(|p| p > MAX_OUTPUT_PRICE) {
                log::warn!(
                    "model choice costs more than ${MAX_OUTPUT_PRICE}/M output tokens; model={}",
                    choice.model
                );
                continue;
            }
            choices.push(choice);
        }
        Self {
            admins: admins.iter().map(ToString::to_string).collect(),
            choices,
            routing: HashMap::new(),
            auto: Vec::new(),
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
        self.admins.contains(user)
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
    /// is no longer offered, or made by someone no longer an admin, falls
    /// back to the default.
    pub fn for_user(&self, user: &str, picked: Option<&str>) -> &Model {
        picked
            .filter(|_| self.is_admin(user))
            .and_then(|id| self.find(id))
            .unwrap_or_else(|| self.default_model())
    }

    /// What `user` sees in the picker, with their settings marked.
    pub fn for_picker(&self, user: &str, picked: &Picked) -> Models {
        let admin = self.is_admin(user);
        let choices = if admin {
            self.choices
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
                .collect()
        } else {
            Vec::new()
        };
        let auto_available = admin && !self.auto.is_empty();
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
            name: info.name.clone().unwrap_or_else(|| model.name().to_owned()),
            input_price: per_million(info.pricing.prompt),
            output_price: per_million(info.pricing.completion),
            sees_images: info.capabilities.images,
            model,
        }
    }
}

/// A per-token price as USD per million tokens.
fn per_million(price: Option<Decimal>) -> Option<f64> {
    (price? * Decimal::from(1_000_000)).to_f64()
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADMIN: &str = "admin";

    fn choices() -> ModelChoices {
        let mut models = ModelChoices::only("openrouter:z-ai/glm-5.3-flash".parse().unwrap());
        models.admins.insert(ADMIN.to_owned());
        models.choices.push(Choice {
            model: "openrouter:openai/gpt-6-luna".parse().unwrap(),
            name: "GPT-6 Luna".into(),
            input_price: Some(0.1),
            output_price: Some(0.5),
            sees_images: true,
        });
        models
    }

    #[test]
    fn only_an_admin_pick_counts() {
        let models = choices();
        let luna = Some("openrouter:openai/gpt-6-luna");
        assert_eq!(models.for_user(ADMIN, luna).name(), "openai/gpt-6-luna");
        assert_eq!(
            models.for_user("someone", luna).name(),
            "z-ai/glm-5.3-flash"
        );
    }

    #[test]
    fn a_pick_no_longer_offered_falls_back_to_the_default() {
        let models = choices();
        let gone = Some("openrouter:retired/model");
        assert_eq!(models.for_user(ADMIN, gone).name(), "z-ai/glm-5.3-flash");
    }

    fn picked(model: &Model, auto: bool) -> Picked<'_> {
        Picked {
            model,
            thinking: ReasoningLevel::Low,
            auto,
        }
    }

    #[test]
    fn only_admins_see_choices() {
        let models = choices();
        let default = models.default_model().clone();
        let picked = picked(&default, false);
        assert_eq!(models.for_picker(ADMIN, &picked).choices.len(), 2);
        assert!(models.for_picker("someone", &picked).choices.is_empty());
    }

    #[test]
    fn auto_takes_only_offered_models_in_choice_order() {
        let luna: Model = "openrouter:openai/gpt-6-luna".parse().unwrap();
        let flash: Model = "openrouter:z-ai/glm-5.3-flash".parse().unwrap();
        let gone: Model = "openrouter:retired/model".parse().unwrap();
        let models = choices().with_auto(HashMap::from([
            (luna.clone(), "Demanding work".to_owned()),
            (flash.clone(), "Quick lookups".to_owned()),
            (gone, "Anything".to_owned()),
        ]));
        let auto: Vec<_> = models.auto().iter().map(|a| &a.model).collect();
        assert_eq!(auto, [&flash, &luna]);
        assert_eq!(models.auto()[1].name, "GPT-6 Luna");

        let on = picked(&flash, true);
        let admin = models.for_picker(ADMIN, &on);
        assert!(admin.auto && admin.auto_available);
        let someone = models.for_picker("someone", &on);
        assert!(!someone.auto && !someone.auto_available);
        assert!(!choices().for_picker(ADMIN, &on).auto);
    }

    #[test]
    fn prices_are_per_million_tokens() {
        let per_token: Decimal = "0.0000005".parse().unwrap();
        assert_eq!(per_million(Some(per_token)), Some(0.5));
    }
}
