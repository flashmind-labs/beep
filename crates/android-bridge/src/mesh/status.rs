use super::Connected;
use beep_mesh::DeviceStatus;
use beep_proto::{
    Machine, MachineDisk, MachineEnv, MachineLogin, MachineStats, MachineTool,
    node::{EnvReport, StatsReport, status},
};
use futures::{StreamExt, stream};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::timeout;

#[derive(Default)]
pub(super) struct Snapshot {
    pub offers: Vec<String>,
    pub env: Option<EnvReport>,
    pub stats: Option<StatsReport>,
}

impl Connected {
    pub(super) async fn snapshot(&self, id: &str, stats: bool) -> Snapshot {
        let mut snapshot = Snapshot::default();
        let _ = timeout(Duration::from_secs(3), async {
            let client = self.client(id).await?;
            let (_tx, mut rx) = client.open(status::PROTOCOL).await?;
            let mut offered = false;
            while !offered || snapshot.env.is_none() || (stats && snapshot.stats.is_none()) {
                match rx.recv::<status::Report>().await? {
                    Some(status::Report::Offers(offers)) => {
                        snapshot.offers = offers;
                        offered = true;
                    }
                    Some(status::Report::Env(env)) => snapshot.env = Some(env),
                    Some(status::Report::Stats(stats)) => snapshot.stats = Some(stats),
                    None => break,
                    _ => {}
                }
            }
            Ok::<(), crate::BeepError>(())
        })
        .await;
        snapshot
    }

    pub async fn machines(&self) -> Vec<Machine> {
        stream::iter(
            self.mesh
                .devices()
                .into_iter()
                .filter(|device| device.device.id != self.mesh.me()),
        )
        .map(|device| self.machine(device))
        .buffer_unordered(4)
        .collect()
        .await
    }

    async fn machine(&self, device: DeviceStatus) -> Machine {
        let env = if device.online {
            self.snapshot(&device.device.id.to_string(), false)
                .await
                .env
                .map(machine_env)
        } else {
            None
        };
        Machine {
            id: device.device.id.to_string(),
            name: device.device.name,
            online: device.online,
            seen_at: if device.online {
                SystemTime::now()
            } else {
                device
                    .last_seen
                    .unwrap_or(UNIX_EPOCH + Duration::from_millis(device.device.added_at_ms))
            },
            version: String::new(),
            env,
        }
    }

    pub async fn machine_stats(&self) -> Vec<MachineStats> {
        stream::iter(
            self.mesh
                .devices()
                .into_iter()
                .filter(|device| device.online && device.device.id != self.mesh.me()),
        )
        .map(|device| async move {
            let id = device.device.id.to_string();
            let stats = self.snapshot(&id, true).await.stats?;
            Some(MachineStats {
                id,
                sampled_at: SystemTime::now(),
                disks: stats
                    .disks
                    .into_iter()
                    .map(|disk| MachineDisk {
                        path: disk.path,
                        free_bytes: disk.free_bytes,
                        total_bytes: disk.total_bytes,
                    })
                    .collect(),
                memory_used_mb: stats.memory_used_mb,
                memory_total_mb: stats.memory_total_mb,
                load: stats.load,
            })
        })
        .buffer_unordered(4)
        .filter_map(|stats| async { stats })
        .collect()
        .await
    }
}

fn machine_env(env: EnvReport) -> MachineEnv {
    MachineEnv {
        os: env.os,
        os_version: env.os_version,
        arch: env.arch,
        hostname: env.hostname,
        cpus: env.cpus,
        memory_mb: env.memory_mb,
        home: env.home,
        tools: env
            .tools
            .into_iter()
            .map(|tool| MachineTool {
                name: tool.name,
                version: tool.version,
            })
            .collect(),
        logins: env
            .logins
            .into_iter()
            .map(|login| MachineLogin {
                tool: login.tool,
                signed_in: login.signed_in,
                account: login.account,
            })
            .collect(),
        avds: env.avds,
    }
}
