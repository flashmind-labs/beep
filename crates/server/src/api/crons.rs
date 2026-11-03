//! Routines.

use super::{Reply, found, next_run};
use crate::{cron, protocol::*, router::Cx, store::decode_schedule};

pub(super) async fn list_crons(cx: Cx, _: ListCrons) -> Reply<Vec<Cron>> {
    let tz = cx
        .user
        .get_timezone()
        .await
        .unwrap_or_else(|_| "UTC".into());
    // Recurring first, then one-off, each soonest first; no next run goes last.
    let mut crons: Vec<_> = (cx.user.list_crons().await?.into_iter())
        .map(|c| (next_run(&decode_schedule(c.schedule.clone()), &tz), c))
        .collect();
    crons.sort_by_key(|(next, c)| (c.run_once, next.is_none(), *next));
    Ok(crons
        .into_iter()
        .map(|(next, mut c)| {
            c.next_run_at = next.map(|t| t.to_rfc3339());
            c
        })
        .collect())
}

pub(super) async fn create_cron(cx: Cx, req: CreateCron) -> Reply<String> {
    cx.user
        .create_cron(&req.label, &req.schedule, &req.prompt)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))
}

pub(super) async fn toggle_cron(cx: Cx, ToggleCron { id }: ToggleCron) -> Reply<bool> {
    let enabled = cx.user.toggle_cron(&id).await?;
    enabled.ok_or(ApiError::NotFound {})
}

pub(super) async fn delete_cron(cx: Cx, DeleteCron { id }: DeleteCron) -> Reply<()> {
    found(cx.user.delete_cron(&id).await?)
}

pub(super) async fn update_cron(cx: Cx, req: UpdateCron) -> Reply<()> {
    let label = req.label.as_deref();
    let schedule = req.schedule.as_deref();
    let updated = cx
        .user
        .update_cron(&req.id, req.enabled, label, schedule)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    found(updated)
}

pub(super) async fn run_cron(cx: Cx, RunCron { id }: RunCron) -> Reply<String> {
    let uuid = id.parse().map_err(|_| ApiError::NotFound {})?;
    let job = cx.user.cron_job(uuid).await?.ok_or(ApiError::NotFound {})?;
    let run = cron::run_job(&cx.app, &cx.user, &job, cron::WhenBusy::GiveUp)
        .await?
        .ok_or_else(|| ApiError::Conflict {
            message: "the routine's chat is busy".into(),
        })?;
    // A one-off reminder that ran early is done; leaving it would run it again.
    if job.once {
        cx.user.delete_cron(&id).await?;
    }
    Ok(run.session_id)
}
