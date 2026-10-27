//! Researches as the Mac's research window reads them.

use super::Reply;
use crate::{
    protocol::*,
    research::{Member, Plan, RoleModel, TeamMember},
    router::Cx,
};

pub(super) async fn get_research(
    cx: Cx,
    GetResearch { session_id, after }: GetResearch,
) -> Reply<Option<Research>> {
    let Some(row) = cx.user.research_in(session_id).await? else {
        return Ok(None);
    };
    let store = &cx.app.store;
    let entries = store.research_entries(row.id, after).await?;
    let sources = store.research_sources(row.id).await?;
    let model_name = |model: &RoleModel| {
        cx.app
            .models
            .name_of(&model.model)
            .map_or_else(|| model.model.to_string(), str::to_owned)
    };
    let agent = |seat: TeamMember| ResearchAgent {
        id: seat.member.to_string(),
        name: seat.member.name(),
        role: seat.role.to_owned(),
        model: model_name(seat.model),
        round: seat.round,
    };
    let agents = match (cx.app.models.research(), &row.plan) {
        (Some(config), Some(plan)) => plan.team(&config.lead).into_iter().map(agent).collect(),
        (Some(config), None) => vec![agent(TeamMember {
            member: Member::Lead,
            role: "Plans and coordinates",
            model: &config.lead,
            round: 0,
        })],
        (None, _) => Vec::new(),
    };
    let round = row.plan.as_ref().map_or(1, Plan::round);
    Ok(Some(Research {
        round,
        id: row.id,
        topic: row.topic,
        status: row.status.wire(),
        agents,
        entries: entries
            .iter()
            .map(|entry| ResearchEntry {
                seq: entry.seq,
                agent: entry.member.to_string(),
                body: entry.entry.wire(),
                timestamp: entry.created_at.timestamp_millis(),
            })
            .collect(),
        sources: sources
            .into_iter()
            .map(|source| ResearchSource {
                number: source.number,
                agent: source.member.to_string(),
                url: source.url,
                title: source.title,
                quote: source.quote,
            })
            .collect(),
        draft: row.draft,
        started_at: row.created_at.timestamp_millis(),
        finished_at: row.finished_at.map(|at| at.timestamp_millis()),
    }))
}

pub(super) async fn list_researches(
    cx: Cx,
    ListResearches {}: ListResearches,
) -> Reply<Vec<ResearchSummary>> {
    let listed = cx.user.researches().await?;
    Ok(listed
        .into_iter()
        .map(|listed| {
            let research = listed.research;
            ResearchSummary {
                session_id: research.chat,
                round: research.plan.as_ref().map_or(1, Plan::round),
                topic: research.topic,
                status: research.status.wire(),
                sources: listed.sources,
                started_at: research.created_at.timestamp_millis(),
                finished_at: research.finished_at.map(|at| at.timestamp_millis()),
            }
        })
        .collect())
}

pub(super) async fn stop_research(cx: Cx, StopResearch { session_id }: StopResearch) -> Reply<()> {
    let row = cx
        .user
        .research_in(session_id)
        .await?
        .ok_or(ApiError::NotFound {})?;
    cx.app.stop_team(row.id).await?;
    Ok(())
}

pub(super) async fn get_research_pdf(
    cx: Cx,
    GetResearchPdf { session_id }: GetResearchPdf,
) -> Reply<Upload> {
    let pdf = cx
        .user
        .research_pdf(session_id)
        .await?
        .ok_or(ApiError::NotFound {})?;
    Ok(Upload {
        name: pdf.name,
        mime: "application/pdf".to_owned(),
        data: pdf.data,
    })
}
