//! The team protocol: people with separate meshes share a forge repository's
//! board over `beep-team/1`. Only cards, rosters and asks cross the link.

use iroh::{EndpointAddr, EndpointId};
use std::{fmt, str::FromStr, time::SystemTime};

/// The ALPN of the team link.
pub const ALPN: &[u8] = b"beep-team/1";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TeamError {
    #[error("not a repository: {0:?}")]
    Repo(String),
    #[error("a forge login cannot be empty")]
    Login,
}

wire_structs! {
    /// A forge repository, normalized so that every remote form of it is equal.
    #[derive(Eq, Hash, PartialOrd, Ord)]
    #[serde(try_from = "RawRepo")]
    pub struct TeamRepo {
        /// Lower case, with its port when it has one.
        pub host: String,
        /// `org/name`, lower case, without `.git`.
        pub repo: String,
    }
}

/// A repository as it arrives on the wire, before it is normalized.
#[derive(serde::Deserialize)]
struct RawRepo {
    host: String,
    repo: String,
}

impl TryFrom<RawRepo> for TeamRepo {
    type Error = TeamError;

    fn try_from(raw: RawRepo) -> Result<Self, TeamError> {
        Self::normalize(&raw.host, &raw.repo)
    }
}

impl TeamRepo {
    /// The host without its port: what the forge's web pages and API go by.
    pub fn hostname(&self) -> &str {
        match self.host.rsplit_once(':') {
            Some((name, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
                name
            }
            _ => &self.host,
        }
    }

    fn normalize(host: &str, path: &str) -> Result<Self, TeamError> {
        let path = path.trim_matches('/');
        let path = path.strip_suffix(".git").unwrap_or(path);
        let host = host.trim().to_lowercase();
        let repo = path.to_lowercase();

        if host.is_empty() || !repo.contains('/') || repo.split('/').any(str::is_empty) {
            return Err(TeamError::Repo(format!("{host}/{path}")));
        }

        Ok(Self { host, repo })
    }
}

impl FromStr for TeamRepo {
    type Err = TeamError;

    /// Reads `git@host:org/repo(.git)`, `ssh://git@host/org/repo`,
    /// `https://host/org/repo(.git)(/)` and `host/org/repo`.
    fn from_str(s: &str) -> Result<Self, TeamError> {
        let bad = || TeamError::Repo(s.to_owned());
        let s = s.trim();
        let rest = match s.split_once("://") {
            Some((_, rest)) => rest,
            None => s,
        };
        let rest = rest.split_once('@').map_or(rest, |(_, rest)| rest);
        // Without a scheme, `host:org/repo` is the scp form and `host:2222/org/repo` has a
        // port: the text between the first ':' and the first '/' is all digits, and there
        // is no user, which every scp remote has (`git@github.com:1234/repo`).
        let ported = |host: &str| {
            host.split_once(':').is_none_or(|(_, port)| {
                !s.contains('@') && !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit())
            })
        };
        let (host, path) = match rest.split_once('/') {
            Some((host, path)) if ported(host) || s.contains("://") => (host, path),
            _ => rest.split_once(':').ok_or_else(bad)?,
        };

        Self::normalize(host, path).map_err(|_| bad())
    }
}

impl fmt::Display for TeamRepo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}/{}", self.host, self.repo)
    }
}

/// A forge account name.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
#[serde(try_from = "String", into = "String")]
pub struct ForgeLogin(String);

impl TryFrom<String> for ForgeLogin {
    type Error = TeamError;

    fn try_from(s: String) -> Result<Self, TeamError> {
        s.parse()
    }
}

impl From<ForgeLogin> for String {
    fn from(login: ForgeLogin) -> String {
        login.0
    }
}

impl FromStr for ForgeLogin {
    type Err = TeamError;

    fn from_str(s: &str) -> Result<Self, TeamError> {
        match s.trim() {
            "" => Err(TeamError::Login),
            login => Ok(Self(login.to_owned())),
        }
    }
}

impl fmt::Display for ForgeLogin {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

wire_structs! {
    /// A person on a team: a forge login and the mesh that proved it.
    pub struct Member {
        pub login: ForgeLogin,
        pub mesh: EndpointId,
        /// Their devices that take team links.
        pub devices: Vec<EndpointAddr>,
        pub joined_at: SystemTime,
        /// The mesh whose member admitted them, which keeps telling the
        /// others of them; `None` for the founder.
        #[serde(default)]
        pub admitted_by: Option<EndpointId>,
    }

    /// An invite anyone holding its secret can join with until it expires.
    pub struct OpenInvite {
        pub secret_hash: [u8; 32],
        pub expires_at: SystemTime,
    }

    /// A team as a desktop stores it.
    pub struct TeamRecord {
        pub repo: TeamRepo,
        pub members: Vec<Member>,
        pub invites: Vec<OpenInvite>,
        /// Issues assigned to me start a session on their own.
        #[serde(default = "yes")]
        pub pickup: bool,
        /// At most this many automatic sessions; `None` for no limit.
        #[serde(default)]
        pub pickup_limit: Option<u32>,
        /// Teammates' agents may ask mine.
        #[serde(default = "yes")]
        pub asks: bool,
        /// Who was removed or left: a roster that still lists them does not bring them back.
        #[serde(default)]
        pub removed: Vec<Removed>,
    }

    /// A member taken off a team, by the login and mesh they had on it.
    #[derive(Eq, PartialOrd, Ord)]
    pub struct Removed {
        pub login: ForgeLogin,
        pub mesh: EndpointId,
        /// When the removed member had joined: one who joined later was let
        /// back in. `None` keeps them out however they join.
        #[serde(default)]
        pub joined_at: Option<SystemTime>,
    }

    /// A person from another mesh, across every team they share with me.
    pub struct ContactRecord {
        pub mesh: EndpointId,
        pub logins: Vec<(TeamRepo, ForgeLogin)>,
        pub devices: Vec<EndpointAddr>,
    }

    /// One agent session on a team's board.
    pub struct Card {
        pub id: String,
        pub title: String,
        pub lane: Lane,
        pub login: ForgeLogin,
        /// The machine's display name.
        pub machine: String,
        pub issue: Option<u64>,
        pub pr: Option<u64>,
        pub updated_at: SystemTime,
    }

    /// The agent session an ask comes from.
    pub struct AgentRef {
        pub login: ForgeLogin,
        pub card: String,
    }
}

fn yes() -> bool {
    true
}

impl TeamRecord {
    /// A team as it starts: these members, no invites, the defaults.
    pub fn new(repo: TeamRepo, members: Vec<Member>) -> Self {
        Self {
            repo,
            members,
            invites: Vec::new(),
            pickup: true,
            pickup_limit: None,
            asks: true,
            removed: Vec::new(),
        }
    }

    /// Takes `login` off the team, keeping the login and mesh they had in
    /// `removed` so that a roster still listing them does not bring them back.
    pub fn remove(&mut self, login: &ForgeLogin) {
        let (gone, kept) = std::mem::take(&mut self.members)
            .into_iter()
            .partition(|member| member.login == *login);
        self.members = kept;

        for member in gone {
            let removed = Removed {
                login: member.login,
                mesh: member.mesh,
                joined_at: Some(member.joined_at),
            };
            self.removed.push(removed);
        }
    }

    /// Takes `changes`, another record's membership, into this one: every
    /// member either lists, with the later join of the two where both list
    /// them, and every removal either holds. A member missing from `changes`
    /// stays: only a removal takes someone off, and a member who joined after
    /// it ends it. An entry whose login or mesh another member holds here is
    /// left out.
    pub fn merge(&mut self, changes: TeamRecord) {
        for gone in changes.removed {
            if !self.removed.contains(&gone) {
                self.removed.push(gone);
            }
        }

        for entry in changes.members {
            let held = self
                .members
                .iter()
                .position(|member| member.login == entry.login || member.mesh == entry.mesh);

            match held {
                None => self.members.push(entry),
                Some(at)
                    if self.members[at].login == entry.login
                        && self.members[at].mesh == entry.mesh
                        && self.members[at].joined_at <= entry.joined_at =>
                {
                    self.members[at] = entry;
                }
                Some(_) => {}
            }
        }

        let removed = &self.removed;
        self.members
            .retain(|member| !removed.iter().any(|gone| gone.covers(member)));
        let members = &self.members;
        self.removed.retain(|gone| {
            let mut members = members.iter();
            !members.any(|member| member.login == gone.login && member.mesh == gone.mesh)
        });
    }

    /// What this record changed from `base`: the entries `base` does not hold
    /// just so, and the removals it does not have. Merged into `base`, it
    /// gives this record's membership back.
    pub fn since(&self, base: &TeamRecord) -> TeamRecord {
        TeamRecord {
            members: self
                .members
                .iter()
                .filter(|member| !base.members.contains(member))
                .cloned()
                .collect(),
            removed: self
                .removed
                .iter()
                .filter(|gone| !base.removed.contains(gone))
                .cloned()
                .collect(),
            ..self.clone()
        }
    }
}

impl Removed {
    /// Whether this removal keeps `member` off: the same login and mesh,
    /// joined no later than the member removed.
    pub fn covers(&self, member: &Member) -> bool {
        self.login == member.login
            && self.mesh == member.mesh
            && self.joined_at.is_none_or(|at| member.joined_at <= at)
    }
}

wire_enum! {
    /// A column of the team board.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::Display, strum::EnumString, strum::IntoStaticStr)]
    #[strum(serialize_all = "snake_case")]
    pub enum Lane {
        0 NeedsYou {}
        1 Working {}
        2 ReadyToReview {}
        3 DoneToday {}
        4 Queued {}
    }
}

wire_enum! {
    /// Who an ask is for: one agent by card id, or a person's own pick.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Target {
        0 Agent { card: String }
        1 Person { login: ForgeLogin }
    }
}

wire_enum! {
    /// A message on the team link.
    #[derive(Debug, Clone, PartialEq)]
    pub enum TeamMsg {
        0 Join { secret: [u8; 32], repo: TeamRepo, login: ForgeLogin, mesh: EndpointId, devices: Vec<EndpointAddr> }
        1 Challenge { nonce: String }
        2 Proved { nonce: String }
        3 Roster { repo: TeamRepo, members: Vec<Member> }
        4 Invite { repo: TeamRepo, ticket: String }
        5 Subscribe { repo: TeamRepo }
        6 Board { repo: TeamRepo, cards: Vec<Card> }
        7 BoardChange { repo: TeamRepo, card: Card, gone: bool }
        8 Ask { id: String, repo: TeamRepo, from: AgentRef, to: Target, text: String, reply_to: Option<String> }
        9 Reply { id: String, text: String }
        10 Leave { repo: TeamRepo, login: ForgeLogin }
        11 Remove { repo: TeamRepo, login: ForgeLogin }
        12 Refused { reason: String }
    }
}

#[cfg(test)]
#[path = "tests/team.rs"]
mod tests;
