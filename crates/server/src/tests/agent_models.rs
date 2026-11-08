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
