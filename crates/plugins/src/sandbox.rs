use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpu_time::ThreadTime;
use log::Level;
use rquickjs::{
    CatchResultExt, Context, Ctx, Function, Module, Object, Persistent, Runtime, Value,
};
use thiserror::Error;

use crate::asset::{Asset, Assets, ImageError};
use crate::console::Console;
use crate::event::UiEvent;
use crate::ident::{Permission, PluginId, Slot};
use crate::instance::Instance;
use crate::lock::locked;
use crate::manifest::{Manifest, ManifestError};
use crate::meter::Meter;
use crate::source::{Imports, Sources};
use crate::store::{DATA_DIR, Store};
use crate::tool::{ToolCall, Tools};
use crate::topic::{Delivery, Subscriptions, Topic};
use crate::ui::{
    MAX_FRAMES, Motion, Node, Placement, RawNode, RawPlace, RawPlacement, RawRepeat, RawStep,
    Repeat, SpriteError, Step, check_motions,
};

const PRELUDE: &str = include_str!("prelude.js");

const MEMORY_LIMIT: usize = 16 * 1024 * 1024;
const STACK_LIMIT: usize = 256 * 1024;
/// CPU time one entry into the script may use.
const CPU_BUDGET: Duration = Duration::from_millis(50);
const MAX_TIMERS: usize = 64;
const MIN_TIMER_MS: f64 = 16.0;
const MAX_TIMER_MS: f64 = 86_400_000.0;
/// Rendered placements, over all instances.
const MAX_ENTRIES: usize = 2000;
/// Nodes in one placement.
const MAX_NODES: usize = 64;
/// One placement, as JSON.
const MAX_RENDER_BYTES: usize = 64 * 1024;
/// All placements of one render, as JSON.
const MAX_TOTAL_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Error)]
pub(crate) enum SandboxError {
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error("cannot read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("script error: {0}")]
    Script(String),
    #[error("script engine error: {0}")]
    Engine(#[from] rquickjs::Error),
    #[error("placed {0} nodes, the limit is {MAX_NODES}")]
    TooManyNodes(usize),
    #[error("rendered {0} placements, the limit is {MAX_ENTRIES}")]
    TooManyPlacements(usize),
    #[error("rendered {0} bytes, the limit is {MAX_RENDER_BYTES} for one placement")]
    TooLarge(usize),
    #[error("rendered {0} bytes in all, the limit is {MAX_TOTAL_BYTES}")]
    TooLargeInAll(usize),
    #[error("the {0} slot takes no buttons, toggles or inputs")]
    NotInteractive(Slot),
    #[error("invalid tree: {0}")]
    Tree(#[from] serde_json::Error),
    #[error("image {src:?}: {reason}")]
    Image { src: String, reason: ImageError },
    #[error("invalid sprite: {0}")]
    Sprite(#[from] SpriteError),
    #[error("tools declared in the manifest but never registered with beep.tool: {0}")]
    Unregistered(String),
    #[error("used {used_ms} ms of CPU time in the last minute, the budget is {budget_ms} ms")]
    OverBudget { used_ms: u64, budget_ms: u64 },
}

/// One plugin in its own QuickJS runtime. It has no threads or clocks of its
/// own: the caller passes the current time to `load` and `fire_due`.
pub(crate) struct Sandbox {
    // Drop order matters: persistent handles, then the context, then the runtime.
    api: Api,
    context: Context,
    runtime: Runtime,
    id: PluginId,
    timers: Arc<Mutex<Timers>>,
    subscriptions: Arc<Mutex<Subscriptions>>,
    tools: Arc<Mutex<Tools>>,
    budget: Budget,
    meter: Arc<Meter>,
    images: Resolver,
    /// The images the last render used.
    shown_assets: Vec<Asset>,
    store: Arc<Mutex<Store>>,
}

/// The functions the prelude hands back, kept to call into the script.
struct Api {
    fire: Persistent<Function<'static>>,
    tool: Persistent<Function<'static>>,
    emit: Persistent<Function<'static>>,
    instances: Persistent<Function<'static>>,
    invoke: Persistent<Function<'static>>,
    render: Persistent<Function<'static>>,
}

impl Api {
    fn save<'js>(ctx: &Ctx<'js>, api: &Object<'js>) -> Result<Self, SandboxError> {
        let get = |name: &str| -> Result<Persistent<Function<'static>>, SandboxError> {
            let function: Function = caught(ctx, api.get(name))?;
            Ok(Persistent::save(ctx, function))
        };

        Ok(Self {
            fire: get("fire")?,
            tool: get("tool")?,
            emit: get("emit")?,
            instances: get("instances")?,
            invoke: get("invoke")?,
            render: get("render")?,
        })
    }
}

struct Timers {
    now: Instant,
    pending: Vec<(u32, Instant)>,
}

/// CPU time the calling thread has used since `start`. A plugin waiting for a
/// busy machine is not charged for the wait.
struct Cpu(Option<ThreadTime>);

impl Cpu {
    fn start() -> Self {
        Self(ThreadTime::try_now().ok())
    }

    /// `None` when the thread's clock cannot be read.
    fn elapsed(&self) -> Option<Duration> {
        self.0.as_ref().map(ThreadTime::elapsed)
    }

    /// Time to record, zero when the clock cannot be read.
    fn spent(&self) -> Duration {
        self.elapsed().unwrap_or_default()
    }
}

/// CPU time an entry into the script may take, enforced by the interrupt
/// handler. The handler runs on the thread that entered the script, so the
/// thread's CPU clock is the right one.
#[derive(Clone, Default)]
struct Budget(Arc<Mutex<Option<Cpu>>>);

impl Budget {
    fn start(&self) {
        *locked(&self.0) = Some(Cpu::start());
    }

    fn stop(&self) {
        *locked(&self.0) = None;
    }

    /// An unreadable clock counts as exceeded, so a loop cannot run unchecked.
    fn exceeded(&self) -> bool {
        locked(&self.0)
            .as_ref()
            .is_some_and(|cpu| cpu.elapsed().is_none_or(|used| used > CPU_BUDGET))
    }
}

impl Sandbox {
    /// Reads the plugin folder and runs its compiled JavaScript entry.
    #[cfg(test)]
    pub(crate) fn load(dir: &Path, now: Instant) -> Result<Self, SandboxError> {
        Self::load_metered(dir, now, Arc::new(Meter::new(crate::meter::BUDGET)))
    }

    /// Like `load`, counting the plugin's CPU time in `meter`.
    pub(crate) fn load_metered(
        dir: &Path,
        now: Instant,
        meter: Arc<Meter>,
    ) -> Result<Self, SandboxError> {
        let manifest = Manifest::read(dir)?;
        let root = dir.canonicalize().map_err(|source| SandboxError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let runtime = Runtime::new()?;
        runtime.set_memory_limit(MEMORY_LIMIT);
        runtime.set_max_stack_size(STACK_LIMIT);
        runtime.set_loader(Imports, Sources::new(root.clone()));
        let budget = Budget::default();
        let watched = budget.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || watched.exceeded())));
        let context = Context::full(&runtime)?;
        let timers = Arc::new(Mutex::new(Timers {
            now,
            pending: Vec::new(),
        }));

        let subscriptions = Arc::new(Mutex::new(Subscriptions::new(&manifest.permissions)));

        let data = root.parent().unwrap_or(&root).join(DATA_DIR);
        let store = Arc::new(Mutex::new(Store::load(
            manifest.id.clone(),
            data.join(format!("{}.json", manifest.id)),
        )));
        let console = Arc::new(Mutex::new(Console::new(manifest.id.clone())));
        let tools = Arc::new(Mutex::new(Tools::new(&manifest.tools)));

        let started = Cpu::start();
        budget.start();
        let can_place = manifest.allows(Permission::UiAdd);
        let entry = format!("./{}", manifest.entry.to_string_lossy().replace('\\', "/"));
        let loaded = context.with(|ctx| {
            let schedule = schedule_function(&ctx, &timers)?;
            let (listen, unlisten) = subscribe_functions(&ctx, &subscriptions)?;
            let check_place = Function::new(ctx.clone(), check_place)?;
            let native = native_functions(&ctx, &store, &console, &tools)?;
            let factory: Function = caught(&ctx, ctx.eval(PRELUDE))?;
            let api: Object = caught(
                &ctx,
                factory.call((schedule, can_place, listen, unlisten, check_place, native)),
            )?;
            let api = Api::save(&ctx, &api)?;

            // The entry and what it imports run now, before the first render.
            let imported = caught(&ctx, Module::import(&ctx, entry))?;
            caught(&ctx, imported.finish::<Value>())?;

            Ok::<_, SandboxError>(api)
        });
        let drained = drain_jobs(&runtime);
        budget.stop();
        meter.record(Instant::now(), started.spent());
        let api = loaded?;
        drained?;
        check_meter(&meter)?;
        if let Some(missing) = locked(&tools).unregistered() {
            return Err(SandboxError::Unregistered(missing));
        }

        Ok(Self {
            api,
            context,
            runtime,
            id: manifest.id,
            timers,
            subscriptions,
            tools,
            budget,
            meter,
            images: Resolver::new(Assets::new(root)),
            shown_assets: Vec::new(),
            store,
        })
    }

    /// When the earliest pending timer is due, or a tool call's time runs out,
    /// or the store's changes are.
    pub(crate) fn next_timer(&self) -> Option<Instant> {
        let timer = locked(&self.timers)
            .pending
            .iter()
            .map(|(_, due)| *due)
            .min();

        [
            timer,
            locked(&self.tools).next_due(),
            locked(&self.store).due(),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Runs every timer due at `now`, in due order, and writes the store if its
    /// changes are due. Timers they set land after `now`.
    pub(crate) fn fire_due(&mut self, now: Instant) -> Result<(), SandboxError> {
        locked(&self.store).flush_due(now);
        locked(&self.tools).expire(now);

        let due = {
            let mut timers = locked(&self.timers);
            timers.now = now;

            let mut due: Vec<(u32, Instant)> = timers
                .pending
                .iter()
                .filter(|(_, at)| *at <= now)
                .copied()
                .collect();
            timers.pending.retain(|(_, at)| *at > now);
            due.sort_by_key(|(id, at)| (*at, *id));
            due
        };

        for (id, _) in due {
            self.enter(|ctx| {
                let fire = self.api.fire.clone().restore(ctx)?;
                caught(ctx, fire.call::<_, ()>((id,)))
            })?;
        }

        Ok(())
    }

    /// Runs a tool's handler. A value or a settled promise answers the call at
    /// once; a pending promise answers when it settles, or times out.
    pub(crate) fn call_tool(&mut self, call: ToolCall, now: Instant) -> Result<(), SandboxError> {
        let name = call.name.to_string();
        let input = call.input.clone();
        let id = locked(&self.tools).start(call, now);

        self.enter(|ctx| {
            let run = self.api.tool.clone().restore(ctx)?;
            caught(ctx, run.call::<_, ()>((id, name, input)))
        })
    }

    /// Hands an update to the callbacks subscribed to its topic.
    pub(crate) fn deliver(&mut self, delivery: &Delivery) -> Result<(), SandboxError> {
        self.enter(|ctx| {
            let emit = self.api.emit.clone().restore(ctx)?;
            caught(ctx, emit.call::<_, ()>((delivery.json(),)))
        })
    }

    /// The distinct topics the plugin follows, once after each change.
    pub(crate) fn take_wanted(&mut self) -> Option<Vec<Topic>> {
        locked(&self.subscriptions).take_changed()
    }

    /// Tells the plugin which instances of its slots are on the page.
    pub(crate) fn set_instances(&mut self, instances: &[Instance]) -> Result<(), SandboxError> {
        let json = serde_json::to_string(instances)?;

        self.enter(|ctx| {
            let set = self.api.instances.clone().restore(ctx)?;
            caught(ctx, set.call::<_, ()>((json,)))
        })
    }

    /// Runs the callback an event names. A callback from an older render, or one
    /// that takes another kind of value, is ignored.
    pub(crate) fn handle(&mut self, event: &UiEvent) -> Result<(), SandboxError> {
        let id = Into::<f64>::into(event.id);
        let argument = event.action.argument();

        self.enter(|ctx| {
            let invoke = self.api.invoke.clone().restore(ctx)?;
            caught(ctx, invoke.call::<_, bool>((id, argument)))
        })?;

        Ok(())
    }

    /// The placements to show, or `None` when nothing changed since the last call.
    pub(crate) fn render(&mut self) -> Result<Option<Vec<Placement>>, SandboxError> {
        let entries = self.enter(|ctx| {
            let render = self.api.render.clone().restore(ctx)?;
            caught(ctx, render.call::<_, Option<Vec<String>>>(()))
        })?;
        let Some(entries) = entries else {
            return Ok(None);
        };

        if entries.len() > MAX_ENTRIES {
            return Err(SandboxError::TooManyPlacements(entries.len()));
        }
        let total: usize = entries.iter().map(String::len).sum();
        if total > MAX_TOTAL_BYTES {
            return Err(SandboxError::TooLargeInAll(total));
        }

        let mut placements = Vec::with_capacity(entries.len());
        let mut used = Used::default();
        for json in &entries {
            if json.len() > MAX_RENDER_BYTES {
                return Err(SandboxError::TooLarge(json.len()));
            }

            let raw: RawPlacement = serde_json::from_str(json)?;
            let slot = raw.place.slot();
            let (enter, leave) = raw.place.transitions();
            used.nodes = 0;
            let node = self.images.resolve(raw.node, &mut used)?;
            if !slot.is_interactive() && node.is_interactive() {
                return Err(SandboxError::NotInteractive(slot));
            }

            placements.push(Placement {
                plugin: self.id.clone(),
                slot,
                instance: raw.instance,
                position: raw.place.position(),
                enter,
                leave,
                node,
            });
        }

        self.shown_assets = used.assets.into_values().collect();

        Ok(Some(placements))
    }

    /// The images the last render used, each once.
    pub(crate) fn assets(&self) -> &[Asset] {
        &self.shown_assets
    }

    /// Runs script code under the CPU budget, then drains promise jobs.
    fn enter<R>(
        &self,
        work: impl for<'js> FnOnce(&Ctx<'js>) -> Result<R, SandboxError>,
    ) -> Result<R, SandboxError> {
        let started = Cpu::start();
        self.budget.start();
        let result = self.context.with(|ctx| work(&ctx));
        let drained = drain_jobs(&self.runtime);
        self.budget.stop();
        self.meter.record(Instant::now(), started.spent());

        let value = result?;
        drained?;
        check_meter(&self.meter)?;

        Ok(value)
    }
}

/// The native functions behind `console`, `beep.log`, `beep.store` and `beep.tool`.
fn native_functions<'js>(
    ctx: &Ctx<'js>,
    store: &Arc<Mutex<Store>>,
    console: &Arc<Mutex<Console>>,
    tools: &Arc<Mutex<Tools>>,
) -> Result<Object<'js>, SandboxError> {
    let console = Arc::clone(console);
    let log = move |level: String, text: String| {
        let level = level.parse().unwrap_or(Level::Info);
        locked(&console).line(Instant::now(), level, &text);
    };
    let held = Arc::clone(store);
    let get = move |key: String| locked(&held).get(&key);
    let held = Arc::clone(store);
    let set = move |key: String, json: String| -> String {
        match locked(&held).set(&key, &json, Instant::now()) {
            Ok(()) => String::new(),
            Err(error) => error.to_string(),
        }
    };
    let held = Arc::clone(store);
    let remove = move |key: String| locked(&held).delete(&key, Instant::now());

    let held = Arc::clone(tools);
    let tool = move |name: String| locked(&held).register(&name);
    let held = Arc::clone(tools);
    let settle = move |id: u32, ok: bool, text: String| locked(&held).settle(id, ok, text);

    let native = Object::new(ctx.clone())?;
    native.set("log", Function::new(ctx.clone(), log)?)?;
    native.set("get", Function::new(ctx.clone(), get)?)?;
    native.set("set", Function::new(ctx.clone(), set)?)?;
    native.set("remove", Function::new(ctx.clone(), remove)?)?;
    native.set("tool", Function::new(ctx.clone(), tool)?)?;
    native.set("settle", Function::new(ctx.clone(), settle)?)?;
    native.set("tools_available", true)?;

    Ok(native)
}

/// The one native function plugins reach, through the prelude's closures.
fn schedule_function<'js>(
    ctx: &Ctx<'js>,
    timers: &Arc<Mutex<Timers>>,
) -> Result<Function<'js>, SandboxError> {
    let timers = Arc::clone(timers);
    let schedule = move |id: u32, ms: f64| -> bool {
        let mut timers = locked(&timers);
        timers.pending.retain(|(pending, _)| *pending != id);
        if ms < 0.0 {
            return true;
        }
        if timers.pending.len() >= MAX_TIMERS {
            return false;
        }

        let ms = if ms.is_nan() { 0.0 } else { ms };
        let delay = Duration::from_millis(ms.clamp(MIN_TIMER_MS, MAX_TIMER_MS) as u64);
        let due = timers.now + delay;
        timers.pending.push((id, due));

        true
    };

    Ok(Function::new(ctx.clone(), schedule)?)
}

/// The native function behind `beep.place`: "" when the options are valid, else why not.
fn check_place(options: String) -> String {
    match serde_json::from_str::<RawPlace>(&options) {
        Ok(_) => String::new(),
        Err(error) => format!("invalid beep.place options: {error}"),
    }
}

/// The two native functions behind `beep.on`: subscribe and drop. `listen`
/// returns the reason a subscription is refused, or an empty string.
fn subscribe_functions<'js>(
    ctx: &Ctx<'js>,
    subscriptions: &Arc<Mutex<Subscriptions>>,
) -> Result<(Function<'js>, Function<'js>), SandboxError> {
    let held = Arc::clone(subscriptions);
    let listen = move |id: u32, topic: String| -> String {
        match locked(&held).add(id, &topic) {
            Ok(()) => String::new(),
            Err(error) => error.to_string(),
        }
    };
    let held = Arc::clone(subscriptions);
    let unlisten = move |id: u32| locked(&held).remove(id);

    Ok((
        Function::new(ctx.clone(), listen)?,
        Function::new(ctx.clone(), unlisten)?,
    ))
}

/// Fails when the plugin used more CPU time than its budget in the last minute.
fn check_meter(meter: &Meter) -> Result<(), SandboxError> {
    let used = meter.used(Instant::now());
    if used > meter.budget() {
        return Err(SandboxError::OverBudget {
            used_ms: u64::try_from(used.as_millis()).unwrap_or(u64::MAX),
            budget_ms: u64::try_from(meter.budget().as_millis()).unwrap_or(u64::MAX),
        });
    }

    Ok(())
}

fn drain_jobs(runtime: &Runtime) -> Result<(), SandboxError> {
    while runtime.is_job_pending() {
        runtime
            .execute_pending_job()
            .map_err(|_| SandboxError::Script("a promise job threw".to_owned()))?;
    }

    Ok(())
}

fn caught<T>(ctx: &Ctx<'_>, result: rquickjs::Result<T>) -> Result<T, SandboxError> {
    result
        .catch(ctx)
        .map_err(|error| SandboxError::Script(error.to_string()))
}

impl Drop for Sandbox {
    /// Writes the store's changes when the plugin stops, reloads or fails.
    fn drop(&mut self) {
        locked(&self.store).flush();
    }
}

/// What a render has used so far: nodes, and the images they name.
#[derive(Default)]
struct Used {
    nodes: usize,
    assets: BTreeMap<crate::asset::AssetHash, Asset>,
}

/// Turns the raw tree a plugin drew into nodes, naming each image file by its hash.
struct Resolver {
    assets: Assets,
}

impl Resolver {
    fn new(assets: Assets) -> Self {
        Self { assets }
    }

    fn resolve(&mut self, raw: RawNode, used: &mut Used) -> Result<Node, SandboxError> {
        used.nodes += 1;
        if used.nodes > MAX_NODES {
            return Err(SandboxError::TooManyNodes(used.nodes));
        }

        Ok(match raw {
            RawNode::Image {
                src,
                width,
                height,
                pixelated,
            } => Node::Image {
                asset: self.asset(&src, used)?.reference(),
                width,
                height,
                pixelated,
            },
            RawNode::Sprite {
                rest,
                width,
                height,
                pixelated,
                motions,
                reactions,
                play,
            } => {
                check_motions(&motions, &reactions, play.as_ref())?;

                let mut paths = vec![rest.as_str()];
                let steps = motions
                    .iter()
                    .flat_map(|motion| &motion.steps)
                    .chain(reactions.values().flat_map(|reaction| &reaction.steps));
                for step in steps {
                    if !paths.contains(&step.frame.as_str()) {
                        paths.push(&step.frame);
                    }
                }
                if paths.len() > MAX_FRAMES {
                    return Err(SpriteError::TooManyFrames(paths.len()).into());
                }

                let mut frames = Vec::with_capacity(paths.len());
                for path in &paths {
                    frames.push(self.asset(path, used)?.reference());
                }
                // Every step's path is in `paths`, and there are at most `MAX_FRAMES`.
                let index = |path: &str| {
                    paths
                        .iter()
                        .position(|known| *known == path)
                        .and_then(|at| u16::try_from(at).ok())
                        .unwrap_or(0)
                };
                let steps = |steps: &[RawStep]| -> Vec<Step> {
                    steps
                        .iter()
                        .map(|step| Step {
                            frame: index(&step.frame),
                            ms: step.ms,
                        })
                        .collect()
                };
                let motions = motions
                    .iter()
                    .map(|motion| Motion {
                        steps: steps(&motion.steps),
                        repeat: match &motion.repeat {
                            RawRepeat::Loop => Repeat::Loop,
                            RawRepeat::Once => Repeat::Once,
                            RawRepeat::Every(range) => Repeat::Every {
                                min_ms: range.min,
                                max_ms: range.max,
                            },
                        },
                    })
                    .collect();
                let reactions = reactions
                    .iter()
                    .map(|(name, reaction)| (name.clone(), steps(&reaction.steps)))
                    .collect();

                Node::Sprite {
                    frames,
                    rest: 0,
                    width,
                    height,
                    pixelated,
                    motions,
                    reactions,
                    play,
                }
            }
            RawNode::Text { text, tone } => Node::Text { text, tone },
            RawNode::Row { gap, children } => Node::Row {
                gap,
                children: self.resolve_all(children, used)?,
            },
            RawNode::Button {
                label,
                children,
                on_press,
            } => Node::Button {
                label,
                children: self.resolve_all(children, used)?,
                on_press,
            },
            RawNode::Toggle {
                label,
                value,
                on_change,
            } => Node::Toggle {
                label,
                value,
                on_change,
            },
            RawNode::Input {
                label,
                value,
                placeholder,
                on_change,
                on_submit,
            } => Node::Input {
                label,
                value,
                placeholder,
                on_change,
                on_submit,
            },
        })
    }

    fn resolve_all(
        &mut self,
        children: Vec<RawNode>,
        used: &mut Used,
    ) -> Result<Vec<Node>, SandboxError> {
        children
            .into_iter()
            .map(|child| self.resolve(child, used))
            .collect()
    }

    /// Loads an image, and counts it as used by this render.
    fn asset(&mut self, src: &str, used: &mut Used) -> Result<Asset, SandboxError> {
        let asset = self
            .assets
            .load(src)
            .map_err(|reason| SandboxError::Image {
                src: src.to_owned(),
                reason,
            })?;
        used.assets
            .entry(asset.hash().clone())
            .or_insert_with(|| asset.clone());

        Ok(asset)
    }
}

#[cfg(test)]
#[path = "tests/sandbox.rs"]
mod tests;
