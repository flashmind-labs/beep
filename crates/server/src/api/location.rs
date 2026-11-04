//! Location, saved places and the time zone.

use super::{Reply, found};
use crate::{
    api::router::Cx,
    protocol::*,
    routines::places,
    routines::wake,
    store::{Location, SavedPlace},
};
use chrono::{DateTime, SecondsFormat, Utc};
use chrono_tz::Tz;
use uuid::Uuid;

pub(super) async fn update_location(cx: Cx, req: UpdateLocation) -> Reply<()> {
    let UpdateLocation {
        latitude,
        longitude,
        accuracy,
        timezone,
        place,
    } = req;
    let valid = latitude.is_finite()
        && longitude.is_finite()
        && accuracy.is_finite()
        && (-90.0..=90.0).contains(&latitude)
        && (-180.0..=180.0).contains(&longitude);
    if !valid {
        return Err(ApiError::bad_request("invalid coordinates"));
    }
    let place = place
        .map(|p| p.trim().chars().take(200).collect::<String>())
        .filter(|p| !p.is_empty());
    let fix = Location {
        latitude,
        longitude,
        accuracy,
        timezone,
        place,
        updated_at: Utc::now(),
    };
    let before = cx.user.get_location().await?;
    cx.user.update_location(&fix).await?;
    // A failure here must not lose the fix, which is already saved.
    if let Err(error) = places::track(&cx.app, &cx.user, &fix).await {
        log::warn!(
            "places: unable to check the fix: {error:#}; user={}",
            cx.user.id()
        );
    }
    if let Some(event) = before.and_then(|before| wake::arrival(&before, &fix)) {
        log::info!("wake: arrival; user={}", cx.user.id());
        let (app, user) = (cx.app.clone(), cx.user.clone());
        tokio::spawn(async move {
            if let Err(error) = wake::wake(&app, &user, event).await {
                log::warn!("wake: unable to start the run: {error:#}; user={user}");
            }
        });
    }
    Ok(())
}

fn place_info(place: SavedPlace) -> Place {
    let time = |at: DateTime<Utc>| at.to_rfc3339_opts(SecondsFormat::Secs, true);
    Place {
        id: place.id.to_string(),
        name: place.name,
        latitude: place.latitude,
        longitude: place.longitude,
        radius: place.radius,
        address: place.address,
        created_at: time(place.created_at),
        here: place.inside,
        last_here_at: place.last_here_at.map(time),
        routines: place.routines,
    }
}

pub(super) async fn list_places(cx: Cx, _: ListPlaces) -> Reply<Vec<Place>> {
    let places = cx.user.list_places().await?;
    Ok(places.into_iter().map(place_info).collect())
}

pub(super) async fn delete_place(cx: Cx, DeletePlace { id }: DeletePlace) -> Reply<()> {
    let id: Uuid = id.parse().map_err(|_| ApiError::NotFound {})?;
    let deleted = cx.user.delete_place(id).await?;
    if deleted {
        // Its routines went with it.
        cx.user.changed("");
    }
    found(deleted)
}

pub(super) async fn update_place(cx: Cx, req: UpdatePlace) -> Reply<Place> {
    let id: Uuid = req.id.parse().map_err(|_| ApiError::NotFound {})?;
    let place = cx
        .user
        .update_place(id, req.name.as_deref(), req.radius)
        .await
        .map_err(|e| ApiError::bad_request(e.to_string()))?;
    place.map(place_info).ok_or(ApiError::NotFound {})
}

pub(super) async fn cross_place(cx: Cx, CrossPlace { id, crossing }: CrossPlace) -> Reply<bool> {
    let id: Uuid = id.parse().map_err(|_| ApiError::NotFound {})?;
    let changed = places::cross(&cx.app, &cx.user, id, crossing.into()).await?;
    if changed {
        cx.user.changed("");
    }
    Ok(changed)
}

pub(super) async fn set_timezone(cx: Cx, SetTimezone { timezone }: SetTimezone) -> Reply<()> {
    let timezone: Tz = timezone
        .parse()
        .map_err(|_| ApiError::bad_request("unknown timezone"))?;
    cx.user.set_timezone(timezone.name()).await?;
    Ok(())
}
