//! The models users can pick for their turns; the server default otherwise.

use crate::{
    config::ProjectConfig,
    protocol::{ModelChoice, Models},
    research::ResearchConfig,
};
use flashmind::types::{LlmProvider, Model, ModelInfo, ProviderPreferences, ReasoningLevel};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use std::{collections::HashMap, iter};

pub struct ModelChoices {
    /// The server default first, then the configured choices.
    choices: Vec<Choice>,
    /// Provider routing for models that do not use the server's own.
    routing: HashMap<Model, ProviderPreferences>,
    /// The offered models Jev may pick per prompt, in choice order.
    auto: Vec<AutoModel>,
    /// The models research roles run on; `None` turns research off.
    research: Option<ResearchConfig>,
    /// What project chats plan on; `None` plans on the chat's default.
    project: Option<ProjectConfig>,
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

#[cfg(test)]
impl ModelChoices {
    /// Offers `model` besides the ones already offered.
    pub(crate) fn offering(mut self, model: Model) -> Self {
        self.choices.push(Choice {
            name: model.name().to_owned(),
            model,
            input_price: None,
            output_price: None,
            sees_images: false,
        });
        self
    }
}

impl ModelChoices {
    /// Only the default.
    pub fn only(default: Model) -> Self {
        Self {
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
            project: None,
        }
    }

    /// Looks up names, prices and vision; unlisted choices are dropped, or kept unpriced without a list.
    pub async fn resolve(
        provider: &dyn LlmProvider,
        default: Model,
        configured: Vec<Model>,
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
            choices,
            routing: HashMap::new(),
            auto: Vec::new(),
            research: None,
            project: None,
        }
    }

    /// Routes these models with their own provider preferences.
    pub fn with_routing(mut self, routing: HashMap<Model, ProviderPreferences>) -> Self {
        self.routing = routing;
        self
    }

    /// Lets Jev pick among these models; unoffered ones are dropped with a warning.
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

    /// Turns research on with these role models; an unoffered role model turns it off.
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

    /// Lets project chats plan on `configured`'s planner, when it is offered.
    pub fn with_project(mut self, configured: Option<ProjectConfig>) -> Self {
        let Some(config) = configured else {
            return self;
        };
        if self.find(&config.planner.model.to_string()).is_none() {
            log::warn!(
                "project planner is not a model choice; projects plan on the default; model={}",
                config.planner.model
            );
            return self;
        }
        self.project = Some(config);
        self
    }

    /// What project chats plan on, when it is set up.
    pub fn project(&self) -> Option<&ProjectConfig> {
        self.project.as_ref()
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

    /// Every offered model.
    pub fn all(&self) -> impl Iterator<Item = &Model> {
        self.choices.iter().map(|choice| &choice.model)
    }

    pub fn default_model(&self) -> &Model {
        &self.choices[0].model
    }

    /// The cheapest offered model; unpriced ones last, the default when none is priced.
    pub fn cheapest(&self) -> &Model {
        self.choices
            .iter()
            .filter_map(|choice| {
                let price = choice.input_price? + choice.output_price?;
                Some((price, &choice.model))
            })
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map_or_else(|| self.default_model(), |(_, model)| model)
    }

    /// The offered model with `id`.
    pub fn find(&self, id: &str) -> Option<&Model> {
        self.choices
            .iter()
            .map(|choice| &choice.model)
            .find(|model| model.to_string() == id)
    }

    /// The model for a user's pick, or the default when it is no longer offered.
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

/// A listed name without its maker: "OpenAI: GPT-6 Luna" reads "GPT-6 Luna".
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
            model: "openrouter:openai/gpt-6-luna".parse().unwrap(),
            name: "GPT-6 Luna".into(),
            input_price: Some(0.1),
            output_price: Some(0.5),
            sees_images: true,
        });
        models
    }

    #[test]
    fn anyone_s_pick_counts() {
        let models = choices();
        let luna = Some("openrouter:openai/gpt-6-luna");
        assert_eq!(models.for_user(luna).name(), "openai/gpt-6-luna");
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
        assert_eq!(choices[1].name, "GPT-6 Luna");
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
        let shown = models.for_picker(&on);
        assert!(shown.auto && shown.auto_available);
        let off = models.for_picker(&picked(&flash, false));
        assert!(!off.auto && off.auto_available);
        assert!(!choices().for_picker(&on).auto);
    }

    #[test]
    fn listed_names_drop_their_maker() {
        assert_eq!(short_name("OpenAI: GPT-6 Luna"), "GPT-6 Luna");
        assert_eq!(short_name("GPT-6 Luna"), "GPT-6 Luna");
    }

    #[test]
    fn prices_are_per_million_tokens() {
        let per_token: Decimal = "0.0000005".parse().unwrap();
        assert_eq!(per_million(Some(per_token)), Some(0.5));
    }

    #[test]
    fn the_cheapest_priced_model_wins() {
        let mut models = choices();
        assert_eq!(models.cheapest().name(), "openai/gpt-6-luna");
        models.choices.push(Choice {
            model: "openrouter:anthropic/claude-haiku-5.5".parse().unwrap(),
            name: "Haiku".into(),
            input_price: Some(0.08),
            output_price: Some(0.4),
            sees_images: true,
        });
        assert_eq!(models.cheapest().name(), "anthropic/claude-haiku-5.5");
        let unpriced = ModelChoices::only("openrouter:z-ai/glm-5.3-flash".parse().unwrap());
        assert_eq!(unpriced.cheapest().name(), "z-ai/glm-5.3-flash");
    }
}
