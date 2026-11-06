//! One pull request or issue read in full from the project's GitHub or Forgejo
//! host, for the project page's detail pane.

use super::{fleet::Fleet, issues::IssueError};
use crate::projects::stored::StoredProject;
use crate::{account::user::User, protocol::ThreadDetail, store::ProjectRow};

/// Reads thread `number` of `project` from its host now.
pub(crate) async fn read(
    fleet: &dyn Fleet,
    user: &User,
    project: &ProjectRow,
    number: u64,
) -> Result<ThreadDetail, IssueError> {
    let store = StoredProject::new(user, project);
    Ok(beep_projects::thread_detail::read(&store, fleet, number).await?)
}
