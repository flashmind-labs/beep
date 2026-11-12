use super::*;

#[test]
fn claude_answer_gives_its_windows() {
    let answer = json!({
        "subscription_type": "max",
        "rate_limits_available": true,
        "rate_limits": {
            "five_hour": null,
            "seven_day": {"utilization": 2, "resets_at": "2026-11-10T01:00:00.586471+00:00"},
            "seven_day_opus": {"utilization": 40.5, "resets_at": null},
            "extra_usage": {"is_enabled": false},
            "model_scoped": [
                {"display_name": "Fable", "utilization": 61, "resets_at": null},
                {"display_name": "Opus", "utilization": 40.5, "resets_at": null},
                {"display_name": "Haiku", "utilization": null, "resets_at": null},
            ],
        },
    });
    let plan = claude_plan(&answer, 7).unwrap();
    assert_eq!(plan.plan.as_deref(), Some("max"));
    assert_eq!(plan.at, 7);
    let shown: Vec<(u32, f64, Option<&str>)> = plan
        .windows
        .iter()
        .map(|w| (w.minutes, w.used_percent, w.scope.as_deref()))
        .collect();
    assert_eq!(
        shown,
        [
            (300, 0.0, None),
            (10080, 2.0, None),
            (10080, 40.5, Some("Opus")),
            (10080, 61.0, Some("Fable"))
        ]
    );
    assert_eq!(plan.windows[1].resets_at, Some(1_794_272_400));
    let api_key = json!({"rate_limits_available": false, "rate_limits": null});
    assert_eq!(claude_plan(&api_key, 7), None);
}

/// A CLI that answers a control request as Claude Code does.
#[cfg(unix)]
#[tokio::test]
async fn reads_a_cli_that_answers() {
    let mut command = Command::new("sh");
    command.args([
        "-c",
        r#"read line; echo '{"type":"system"}'; echo '{"type":"control_response","response":{"subtype":"success","request_id":"beep-usage","response":{"subscription_type":"pro","rate_limits_available":true,"rate_limits":{"five_hour":{"utilization":38,"resets_at":null}}}}}'; cat >/dev/null"#,
    ]);
    let plan = read_claude(command).await.unwrap().unwrap();
    assert_eq!(plan.plan.as_deref(), Some("pro"));
    assert_eq!(plan.windows[0].used_percent, 38.0);
}
