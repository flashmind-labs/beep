//! Apps, models, agents and what each app may do.

use super::Reply;
use crate::{
    apps::catalog_app, feeds, models::Picked, permissions, profile, protocol::*, router::Cx,
    store::Feed,
};

pub(super) async fn list_apps(cx: Cx, _: ListApps) -> Reply<Vec<AppIntegration>> {
    let apps = cx.user.apps().await?;
    // OAuth ends in the browser and the phone lists apps again on return,
    // so this is the first moment the server sees a new mailbox or Slack.
    let feeds: Vec<Feed> = apps
        .iter()
        .filter(|app| app.connected)
        .filter_map(|app| Feed::from_slug(&app.slug))
        .collect();
    if !feeds.is_empty() {
        feeds::spawn_sync(&cx.app, &cx.user, feeds);
    }
    let connected: Vec<String> = apps
        .iter()
        .filter(|app| app.connected)
        .map(|app| app.slug.clone())
        .collect();
    profile::spawn(&cx.app, &cx.user, &connected);
    Ok(apps)
}

pub(super) async fn connect_app(cx: Cx, ConnectApp { slug }: ConnectApp) -> Reply<ConnectLink> {
    cx.user.connect_app(&slug).await
}

pub(super) async fn disconnect_app(cx: Cx, DisconnectApp { slug }: DisconnectApp) -> Reply<()> {
    cx.user.disconnect_app(&slug).await
}

pub(super) async fn get_models(cx: Cx, _: GetModels) -> Reply<Models> {
    picker(&cx).await
}

/// The caller's picker with their current settings.
async fn picker(cx: &Cx) -> Reply<Models> {
    let model = cx.app.model_for(&cx.user).await;
    let thinking = cx.app.thinking_for(&cx.user).await;
    let auto = cx.user.auto_model().await?;
    let picked = Picked {
        model: &model,
        thinking,
        auto,
    };
    Ok(cx.app.models.for_picker(cx.user.id(), &picked))
}

pub(super) async fn set_model(cx: Cx, SetModel { id }: SetModel) -> Reply<Models> {
    if !cx.app.models.is_admin(cx.user.id()) {
        return Err(ApiError::Unauthorized {});
    }
    let model = cx
        .app
        .models
        .find(&id)
        .ok_or(ApiError::NotFound {})?
        .clone();
    cx.user.set_model(&model).await?;
    log::info!("model picked; user={} model={model}", cx.user.id());
    picker(&cx).await
}

pub(super) async fn set_thinking(cx: Cx, SetThinking { level }: SetThinking) -> Reply<Models> {
    if !cx.app.models.is_admin(cx.user.id()) {
        return Err(ApiError::Unauthorized {});
    }
    cx.user.set_thinking(level).await?;
    log::info!("thinking picked; user={} level={level}", cx.user.id());
    picker(&cx).await
}

pub(super) async fn set_auto_model(
    cx: Cx,
    SetAutoModel { enabled }: SetAutoModel,
) -> Reply<Models> {
    if !cx.app.models.is_admin(cx.user.id()) {
        return Err(ApiError::Unauthorized {});
    }
    cx.user.set_auto_model(enabled).await?;
    log::info!("auto model set; user={} enabled={enabled}", cx.user.id());
    picker(&cx).await
}

pub(super) async fn get_agents(cx: Cx, _: GetAgents) -> Reply<Agents> {
    if !cx.app.models.is_admin(cx.user.id()) {
        return Err(ApiError::Unauthorized {});
    }
    agents(&cx).await
}

pub(super) async fn set_agents(cx: Cx, SetAgents { enabled }: SetAgents) -> Reply<Agents> {
    if !cx.app.models.is_admin(cx.user.id()) {
        return Err(ApiError::Unauthorized {});
    }
    if enabled && cx.app.models.agents().is_empty() {
        return Err(ApiError::bad_request("the server has no agents set up"));
    }
    cx.user.set_agents_enabled(enabled).await?;
    log::info!("agents set; user={} enabled={enabled}", cx.user.id());
    agents(&cx).await
}

/// Whether agents are on, with the agents the server has.
async fn agents(cx: &Cx) -> Reply<Agents> {
    let agents = cx
        .app
        .models
        .agents()
        .iter()
        .map(|agent| AgentInfo {
            id: agent.id.as_str().to_owned(),
            name: agent.name.clone(),
            when: agent.when.clone(),
            model: agent.model.to_string(),
            thinking: agent.thinking,
        })
        .collect();
    Ok(Agents {
        enabled: cx.app.store.agents_enabled().await?,
        agents,
    })
}

pub(super) async fn get_app_permissions(
    cx: Cx,
    GetAppPermissions { slug }: GetAppPermissions,
) -> Reply<AppPermissions> {
    let app = catalog_app(&slug).ok_or(ApiError::NotFound {})?;
    let rules = cx.user.app_rules().await?;
    Ok(permissions::app_permissions(app, &rules))
}

pub(super) async fn set_app_permissions(cx: Cx, req: SetAppPermissions) -> Reply<()> {
    let app = catalog_app(&req.slug).ok_or(ApiError::NotFound {})?;
    let row = permissions::row(app, req.level, req.allowed);
    Ok(cx.user.save_app_permissions(&row).await?)
}

pub(super) async fn start_app_auth(
    cx: Cx,
    StartAppAuth { slug, redirect_uri }: StartAppAuth,
) -> Reply<ConnectLink> {
    cx.user.start_app_auth(&slug, &redirect_uri).await
}

pub(super) async fn finish_app_auth(cx: Cx, req: FinishAppAuth) -> Reply<()> {
    cx.user
        .finish_app_auth(&req.slug, &req.code, &req.state, req.issuer.as_deref())
        .await
}

pub(super) async fn list_mcp_servers(cx: Cx, _: ListMcpServers) -> Reply<Vec<McpServer>> {
    cx.user.list_custom_mcp().await
}

pub(super) async fn add_mcp_server(
    cx: Cx,
    AddMcpServer { name, url }: AddMcpServer,
) -> Reply<McpConnect> {
    cx.user.add_custom_mcp(&name, &url).await
}

pub(super) async fn connect_mcp_server(
    cx: Cx,
    ConnectMcpServer { id }: ConnectMcpServer,
) -> Reply<McpConnect> {
    cx.user.reconnect_custom_mcp(&id).await
}

pub(super) async fn finish_mcp_auth(cx: Cx, req: FinishMcpAuth) -> Reply<McpServer> {
    cx.user
        .finish_custom_mcp(&req.code, &req.state, req.issuer.as_deref())
        .await
}

pub(super) async fn remove_mcp_server(
    cx: Cx,
    RemoveMcpServer { id }: RemoveMcpServer,
) -> Reply<()> {
    cx.user.remove_custom_mcp(&id).await
}
