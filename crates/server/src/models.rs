//! The models admins can pick for their own turns. Everyone else, and every
//! admin who has not picked one, runs on the server default.

use crate::protocol::{ModelChoice, Models};
use flashmind::types::{LlmProvider, Model, ModelInfo};
use iroh::EndpointId;
use rust_decimal::{Decimal, prelude::ToPrimitive};
use std::{collections::HashSet, iter};

/// The most a pickable model may cost, in USD per million output tokens.
pub const MAX_OUTPUT_PRICE: f64 = 4.0;

pub struct ModelChoices {
    admins: HashSet<String>,
    /// The server default first, then the configured choices.
    choices: Vec<Choice>,
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
        }
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

    /// What `user` sees in the picker, with `selected` marked.
    pub fn for_picker(&self, user: &str, selected: &Model) -> Models {
        let choices = if self.is_admin(user) {
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
        Models {
            choices,
            selected: selected.to_string(),
        }
    }
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

    #[test]
    fn only_admins_see_choices() {
        let models = choices();
        let default = models.default_model().clone();
        assert_eq!(models.for_picker(ADMIN, &default).choices.len(), 2);
        assert!(models.for_picker("someone", &default).choices.is_empty());
    }

    #[test]
    fn prices_are_per_million_tokens() {
        let per_token: Decimal = "0.0000005".parse().unwrap();
        assert_eq!(per_million(Some(per_token)), Some(0.5));
    }
}
