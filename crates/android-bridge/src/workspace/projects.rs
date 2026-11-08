use super::parse_id;
use crate::{BeepClient, BeepError};
use beep_fleet::ImportJob;
use beep_proto::{
    GetBranchDiff, GetProjectAgent, GetProjectLists, GetProjectRun, GetProjectSyncDetail,
    GetPullRequestHead, GetThreadDetail, GetWorkDiff, GitHubAccount, ImportProject,
    ImportedProject, ListGitHubAccounts, ListImportedProjects, ListProjectBranches,
    ListProjectMachines, ListProjectRuns, ListProjects, ListProjectsOnTop, PauseProjectSync,
    PinProjectToTop, ProjectAgent, ProjectBranches, ProjectIssue, ProjectMachine,
    ProjectPullRequest, ProjectRunDetail, ProjectSyncDetail, PullRequestHead, RemoveProject,
    RunState, ThreadDetail, WorkDiff, WorkflowRun,
};
use std::time::SystemTime;

// These view records expose replies whose wire structs are not FFI records.
#[derive(uniffi::Record)]
pub struct MobileProjectLists {
    pub pull_requests: Vec<ProjectPullRequest>,
    pub issues: Vec<ProjectIssue>,
    pub prs_error: Option<String>,
    pub issues_error: Option<String>,
}

#[derive(uniffi::Record)]
pub struct MobileRun {
    pub id: u64,
    pub number: u64,
    pub workflow: String,
    pub title: String,
    pub branch: String,
    pub event: String,
    pub state: RunState,
    pub url: String,
    pub started_at: Option<SystemTime>,
    pub finished_at: Option<SystemTime>,
}

impl From<WorkflowRun> for MobileRun {
    fn from(run: WorkflowRun) -> Self {
        Self {
            id: run.id,
            number: run.number,
            workflow: run.workflow,
            title: run.title,
            branch: run.branch,
            event: run.event,
            state: run.state,
            url: run.url,
            started_at: run.started_at,
            finished_at: run.finished_at,
        }
    }
}

#[derive(uniffi::Record)]
pub struct MobileRuns {
    pub runs: Vec<MobileRun>,
    pub error: Option<String>,
}

#[derive(uniffi::Record)]
pub struct MobileJob {
    pub name: String,
    pub state: RunState,
    pub steps: Vec<MobileStep>,
}

#[derive(uniffi::Record)]
pub struct MobileStep {
    pub name: String,
    pub state: RunState,
}

#[derive(uniffi::Record)]
pub struct MobileRunDetail {
    pub run: MobileRun,
    pub jobs: Vec<MobileJob>,
    pub log: Option<String>,
    pub error: Option<String>,
}

#[uniffi::export(async_runtime = "tokio")]
impl BeepClient {
    pub async fn list_projects(&self) -> Result<Vec<ProjectAgent>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.projects());
        }
        self.call(ListProjects {}).await
    }

    pub async fn list_imported_projects(&self) -> Result<Vec<ImportedProject>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.imported().await);
        }
        self.call(ListImportedProjects {}).await
    }

    pub async fn project_agent(&self, project: String) -> Result<ProjectAgent, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return mesh
                .projects()
                .into_iter()
                .find(|known| known.name == project)
                .ok_or(BeepError::NotFound);
        }
        self.call(GetProjectAgent { project }).await
    }

    pub async fn import_project(
        &self,
        repo: String,
        machine_id: String,
        folder: Option<String>,
    ) -> Result<ImportedProject, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh
                .desk()
                .await?
                .import(ImportJob {
                    repo: &repo,
                    machine_id: Some(&machine_id),
                    folder: folder.as_deref(),
                    parent: None,
                    worktrees: &[],
                })
                .await?);
        }
        self.call(ImportProject {
            repo,
            machine_id: Some(machine_id),
            folder,
            worktrees: Vec::new(),
            parent: None,
        })
        .await
    }

    pub async fn project_lists(
        &self,
        project_id: String,
        refresh: bool,
    ) -> Result<MobileProjectLists, BeepError> {
        let lists = if let Some(mesh) = self.mesh().await? {
            let project = mesh.project_name(&project_id).await?;
            mesh.desk().await?.lists(&project).await?.lists
        } else {
            self.call(GetProjectLists {
                project_id: parse_id(&project_id)?,
                refresh,
            })
            .await?
        };
        Ok(MobileProjectLists {
            pull_requests: lists.pull_requests,
            issues: lists.issues,
            prs_error: lists.prs_error,
            issues_error: lists.issues_error,
        })
    }

    pub async fn project_machines(
        &self,
        project: String,
    ) -> Result<Vec<ProjectMachine>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.project_machines(&project).await?);
        }
        self.call(ListProjectMachines { project }).await
    }

    pub async fn project_sync(&self, project: String) -> Result<ProjectSyncDetail, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.sync_detail(&project).await?);
        }
        self.call(GetProjectSyncDetail { project }).await
    }

    pub async fn project_branches(&self, project: String) -> Result<ProjectBranches, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.branches(&project).await?);
        }
        self.call(ListProjectBranches { project }).await
    }

    pub async fn thread_detail(
        &self,
        project_id: String,
        number: u64,
    ) -> Result<ThreadDetail, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            let project = mesh.project_name(&project_id).await?;
            return Ok(mesh.desk().await?.thread_detail(&project, number).await?);
        }
        self.call(GetThreadDetail {
            project_id: parse_id(&project_id)?,
            number,
        })
        .await
    }

    pub async fn pull_request_head(
        &self,
        project_id: String,
        number: u64,
    ) -> Result<PullRequestHead, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            let project = mesh.project_name(&project_id).await?;
            return Ok(mesh.desk().await?.pr_head(&project, number).await?);
        }
        self.call(GetPullRequestHead {
            project_id: parse_id(&project_id)?,
            number,
        })
        .await
    }

    pub async fn branch_diff(
        &self,
        project: String,
        branch: String,
    ) -> Result<WorkDiff, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            let place = mesh
                .desk()
                .await?
                .project_machines(&project)
                .await?
                .into_iter()
                .find(|place| place.online && place.folder.is_some())
                .ok_or_else(|| crate::bad_request("No checkout is online."))?;
            return mesh
                .diff(
                    &place.machine_id,
                    place.folder.as_deref().unwrap_or(""),
                    &branch,
                )
                .await;
        }
        self.call(GetBranchDiff {
            project,
            branch,
            only: Vec::new(),
            lockfiles: false,
        })
        .await
    }

    pub async fn session_diff(
        &self,
        machine_id: String,
        root: String,
    ) -> Result<WorkDiff, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return mesh.diff(&machine_id, &root, "").await;
        }
        self.call(GetWorkDiff {
            machine_id,
            root,
            base: String::new(),
            head: String::new(),
            only: Vec::new(),
            lockfiles: false,
        })
        .await
    }

    pub async fn project_runs(
        &self,
        project_id: String,
        refresh: bool,
    ) -> Result<MobileRuns, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            let project = mesh.project_name(&project_id).await?;
            let found = mesh.desk().await?.runs(&project).await?;
            return Ok(MobileRuns {
                runs: found.runs.into_iter().map(|run| run.run.into()).collect(),
                error: found.error,
            });
        }
        let runs = self
            .call(ListProjectRuns {
                project_id: parse_id(&project_id)?,
                refresh,
            })
            .await?;
        Ok(MobileRuns {
            runs: runs.runs.into_iter().map(MobileRun::from).collect(),
            error: runs.error,
        })
    }

    pub async fn project_run(
        &self,
        project_id: String,
        run_id: u64,
    ) -> Result<MobileRunDetail, BeepError> {
        let detail = if let Some(mesh) = self.mesh().await? {
            let project = mesh.project_name(&project_id).await?;
            mesh.desk().await?.run(&project, None, run_id).await?
        } else {
            self.call(GetProjectRun {
                project_id: parse_id(&project_id)?,
                run_id,
            })
            .await?
        };
        Ok(MobileRunDetail::from(detail))
    }

    pub async fn github_accounts(&self) -> Result<Vec<GitHubAccount>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.github_accounts());
        }
        self.call(ListGitHubAccounts {}).await
    }

    pub async fn projects_on_top(&self) -> Result<Vec<String>, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.on_top());
        }
        self.call(ListProjectsOnTop {}).await
    }

    pub async fn pin_project(&self, project: String, pinned: bool) -> Result<(), BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.pin(&project, pinned).await?);
        }
        self.call(PinProjectToTop { project, pinned }).await
    }

    pub async fn remove_project(&self, project: String) -> Result<(), BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.remove(&project).await?);
        }
        self.call(RemoveProject { project }).await
    }

    pub async fn pause_project_sync(
        &self,
        project: String,
        paused: bool,
    ) -> Result<ProjectSyncDetail, BeepError> {
        if let Some(mesh) = self.mesh().await? {
            return Ok(mesh.desk().await?.pause_sync(&project, paused).await?);
        }
        self.call(PauseProjectSync { project, paused }).await
    }
}

impl From<ProjectRunDetail> for MobileRunDetail {
    fn from(detail: ProjectRunDetail) -> Self {
        Self {
            run: detail.run.into(),
            jobs: detail
                .jobs
                .into_iter()
                .map(|job| MobileJob {
                    name: job.name,
                    state: job.state,
                    steps: job
                        .steps
                        .into_iter()
                        .map(|step| MobileStep {
                            name: step.name,
                            state: step.state,
                        })
                        .collect(),
                })
                .collect(),
            log: detail.log.map(|log| log.text),
            error: detail.error,
        }
    }
}
