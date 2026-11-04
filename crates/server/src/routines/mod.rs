//! What wakes the agent without a message: scheduled jobs (`cron`), places, account events and the
//! evening digest. `wake` runs the agent in a new chat for any of them.

pub mod cron;
pub mod digest;
pub mod events;
pub mod places;
pub mod wake;
