// Synthetic, obviously fake data for the perf harness, in the shapes the
// desktop reads (see src/chat/chats.ts, lists/memories.ts, machines/,
// center/home-state.ts, project/projects-state.ts). Every name is made up.

export const SIZES = { chats: 2000, memories: 2000, machines: 30, projects: 200, items: 500, sessions: 200 };

const NOW = Date.UTC(2026, 0, 15, 12, 0, 0);
const pad = (n, w = 4) => String(n).padStart(w, "0");
const WORDS = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india", "juliet"];
const word = i => WORDS[i % WORDS.length];

function machines(n) {
  return Array.from({ length: n }, (_, i) => ({
    id: `fake-machine-${pad(i, 2)}`,
    name: `fake-box-${pad(i, 2)}`,
    online: i % 4 !== 3,
    seen_at: NOW - i * 3600_000,
    version: "0.0.0-fake",
    env: i % 10 === 9 ? null : {
      os: i % 2 ? "macos" : "linux",
      os_version: i % 2 ? "macOS 0.0" : "Fakelinux 0.0",
      arch: i % 2 ? "aarch64" : "x86_64",
      hostname: `fake-host-${pad(i, 2)}`,
      cpus: 8 + (i % 4) * 8,
      memory_mb: 16384,
      home: "/fake/home",
      tools: ["git", "node", "claude", "codex", "docker", "cargo", "python3", "just"].map((name, k) => ({ name, version: `0.${k}.${i}` })),
      logins: [{ tool: "gh", signed_in: i % 2 === 0, account: i % 2 === 0 ? "fake-user" : null }],
      avds: [],
    },
  }));
}

function machineCards(n) {
  return machines(n).map((m, i) => ({
    ...m,
    joined: true,
    apps: [{ name: "FakeApp", version: "1.0" }],
    gpus: i % 3 === 0 ? [{ vendor: "FakeVendor", model: "Fake GPU 9000", vram_mb: 8192, driver: null }] : [],
    capabilities: i % 3 === 0 ? ["gpu", "docker"] : ["docker"],
    agents: i % 5,
    stats: m.online ? {
      sampled_at: NOW,
      disks: [{ path: "/fake", free_bytes: (50 + i) * 1e9, total_bytes: 500e9 }],
      memory_used_mb: 4000 + i * 100,
      memory_total_mb: 16384,
      load: 0.5,
    } : null,
  }));
}

export function makeData(sizes = SIZES) {
  const ms = machines(sizes.machines);
  const projects = Array.from({ length: sizes.projects }, (_, i) => {
    const m = ms[i % ms.length];
    return {
      name: `fake-org-${pad(i % 20, 2)}/fake-repo-${pad(i, 3)}`,
      url: `https://git.example.test/fake-org-${pad(i % 20, 2)}/fake-repo-${pad(i, 3)}`,
      machine_id: m.id,
      machine_name: m.name,
      folder: `/fake/work/fake-repo-${pad(i, 3)}`,
      cloned: true,
      imported_at: NOW - i * 60_000,
    };
  });
  const running = Array.from({ length: sizes.sessions }, (_, i) => {
    const p = projects[i % projects.length];
    return {
      kind: "agent",
      machine_id: p.machine_id,
      machine_name: p.machine_name,
      id: `fake-session-${pad(i)}`,
      agent: i % 2 ? "claude" : "codex",
      title: `Fake agent task ${pad(i)}`,
      cwd: p.folder,
      status: i % 3 === 0 ? "working" : "idle",
      doing: null,
      started_at: NOW - i * 120_000,
      last_activity: NOW - i * 60_000,
      external: false,
    };
  });
  const home = {
    machines: machineCards(sizes.machines),
    running,
    needs_you: [],
    coming_up: Array.from({ length: 20 }, (_, i) => ({ id: `fake-cron-${i}`, label: `Fake routine ${i}`, prompt: "fake", schedule: "0 9 * * *", next_run_at: new Date(NOW + i * 3600_000).toISOString() })),
    updates: Array.from({ length: 20 }, (_, i) => ({ id: i + 1, summary: `Fake update ${i}`, app: "fakeapp", app_name: "Fake App", at: NOW - i * 60_000 })),
    projects,
  };
  const items = Array.from({ length: sizes.items }, (_, i) => {
    const p = projects[i % projects.length];
    return {
      host: "git.example.test",
      repo: p.name,
      number: 1 + Math.floor(i / projects.length) * 7 + (i % 7),
      kind: i % 3 === 0 ? "issue" : "pull_request",
      title: `Fake ${i % 3 === 0 ? "issue" : "pull request"} ${pad(i)} about ${word(i)}`,
      url: `${p.url}/pull/${i}`,
      updated_at: NOW - i * 90_000,
      authored: i % 4 === 0,
      assigned: i % 5 === 0,
      review_requested: i % 6 === 0,
      draft: i % 9 === 0,
      review: i % 8 === 0 ? "approved" : "none",
      checks_failing: i % 11 === 0,
      comments: i % 13,
      labels: i % 2 ? ["fake-label"] : [],
      opened_at: NOW - i * 190_000,
    };
  });
  const sessions = Array.from({ length: sizes.chats }, (_, i) => ({
    id: `fake-chat-${pad(i)}`,
    title: `Fake chat ${pad(i)} about ${word(i)} ${word(i * 3)}`,
    pinned: i < 3,
    updated_at: NOW - i * 300_000,
  }));
  const memories = Array.from({ length: sizes.memories }, (_, i) => ({
    id: `fake-memory-${pad(i)}`,
    content: `Fake memory ${pad(i)}: the ${word(i)} widget is kept in the ${word(i * 7)} drawer, as noted on a fake day.`,
    category: `Fake category ${i % 12}`,
    source: ["user", "learned", "email", "slack"][i % 4],
    updated: `2026-01-${pad(1 + (i % 28), 2)}`,
    valid_from: null,
    valid_until: i % 50 === 0 ? "2025-01-01" : null,
    quote: i % 5 === 0 ? `Fake quote ${i} mentioning ${word(i)}` : null,
  }));
  const scopes = memories.filter((_, i) => i % 3 === 0).map((m, k) => ({
    id: m.id,
    scope: k % 2 ? { kind: "project", name: `fake-org-${pad(k % 20, 2)}/fake-repo-${pad(k % 200, 3)}` } : { kind: "machine", id: ms[k % ms.length].id, name: ms[k % ms.length].name },
  }));
  const overview = projects.slice(0, sizes.projects).map((p, i) => ({
    project_id: `fake-project-${pad(i, 3)}`,
    name: p.name,
    tasks: { planned: i % 3, running: i % 2, blocked: 0, review: i % 4, done: i % 10 },
    workers: i % 2,
    latest: null,
  }));
  return {
    status: { link: "linked", signed_out: false },
    mac_control: { id: "fake-mac", name: "Fake Mac", enabled: true, status: "connected", error: null, permissions: {} },
    center_home: home,
    list_sessions: sessions,
    list_groups: [],
    list_memories: memories,
    list_memory_scopes: scopes,
    list_memory_reads: [],
    list_machines: ms,
    updatable_machines: [],
    machine_traffic: [],
    tool_updates: [],
    git_items: { items, hosts: [{ host: "git.example.test", forgejo: false, account: "fake-user", error: null }] },
    project_overview: overview,
    project_groups: [],
    projects_on_top: [],
    project_sync: [],
    found_projects: [],
    agent_hosts: [],
    // A search answers with the first matches, as the server's search does.
    search_sessions: sessions.slice(0, 200).map(session => ({ session, snippet: `fake snippet for ${session.title}`, from_user: true })),
  };
}
