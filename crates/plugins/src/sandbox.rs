use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rquickjs::context::EvalOptions;
use rquickjs::{CatchResultExt, Context, Ctx, Function, Object, Persistent, Runtime};
use thiserror::Error;

use crate::event::UiEvent;
use crate::instance::Instance;
use crate::manifest::{Manifest, ManifestError, Permission, PluginId};
use crate::source::{SourceError, strip_types};
use crate::topic::{Delivery, Subscriptions, Topic};
use crate::ui::{Node, Placement, RawNode, RawPlace, RawPlacement, Slot};

const PRELUDE: &str = include_str!("prelude.js");

const MEMORY_LIMIT: usize = 16 * 1024 * 1024;
const STACK_LIMIT: usize = 256 * 1024;
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
const MAX_IMAGE_BYTES: u64 = 256 * 1024;

#[derive(Debug, Error)]
pub(crate) enum SandboxError {
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error(transparent)]
    Source(#[from] SourceError),
    #[error("cannot read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("entry {} is outside the plugin folder", .0.display())]
    EntryOutside(PathBuf),
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
}

#[derive(Debug, Error)]
pub(crate) enum ImageError {
    #[error("path must be relative")]
    Absolute,
    #[error("path leaves the plugin folder")]
    Outside,
    #[error("cannot read: {0}")]
    Io(#[from] std::io::Error),
    #[error("not an image file")]
    NotImage,
    #[error("{0} bytes is over the {MAX_IMAGE_BYTES} byte limit")]
    TooLarge(u64),
}

/// One plugin in its own QuickJS runtime. It has no threads or clocks of its
/// own: the caller passes the current time to `load` and `fire_due`.
pub(crate) struct Sandbox {
    // Drop order matters: persistent handles, then the context, then the runtime.
    fire: Persistent<Function<'static>>,
    emit: Persistent<Function<'static>>,
    instances: Persistent<Function<'static>>,
    invoke: Persistent<Function<'static>>,
    render: Persistent<Function<'static>>,
    context: Context,
    runtime: Runtime,
    id: PluginId,
    timers: Arc<Mutex<Timers>>,
    subscriptions: Arc<Mutex<Subscriptions>>,
    budget: Budget,
    images: Images,
}

struct Timers {
    now: Instant,
    pending: Vec<(u32, Instant)>,
}

/// Wall time an entry into the script may take, enforced by the interrupt handler.
#[derive(Clone, Default)]
struct Budget(Arc<Mutex<Option<Instant>>>);

impl Budget {
    fn start(&self) {
        *locked(&self.0) = Some(Instant::now() + CPU_BUDGET);
    }

    fn stop(&self) {
        *locked(&self.0) = None;
    }

    fn exceeded(&self) -> bool {
        locked(&self.0).is_some_and(|deadline| Instant::now() > deadline)
    }
}

impl Sandbox {
    /// Reads the plugin folder, strips types from the entry file and runs it.
    pub(crate) fn load(dir: &Path, now: Instant) -> Result<Self, SandboxError> {
        let manifest = Manifest::read(dir)?;
        let root = dir.canonicalize().map_err(|source| SandboxError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let entry_path = root.join(&manifest.entry);
        let entry_real = entry_path
            .canonicalize()
            .map_err(|source| SandboxError::Io {
                path: entry_path,
                source,
            })?;
        if !entry_real.starts_with(&root) {
            return Err(SandboxError::EntryOutside(manifest.entry));
        }
        let source = std::fs::read_to_string(&entry_real).map_err(|source| SandboxError::Io {
            path: entry_real.clone(),
            source,
        })?;
        let script = strip_types(&source, &manifest.entry)?;

        let runtime = Runtime::new()?;
        runtime.set_memory_limit(MEMORY_LIMIT);
        runtime.set_max_stack_size(STACK_LIMIT);
        let budget = Budget::default();
        let watched = budget.clone();
        runtime.set_interrupt_handler(Some(Box::new(move || watched.exceeded())));
        let context = Context::full(&runtime)?;
        let timers = Arc::new(Mutex::new(Timers {
            now,
            pending: Vec::new(),
        }));

        let subscriptions = Arc::new(Mutex::new(Subscriptions::new(&manifest.permissions)));

        budget.start();
        let can_place = manifest.allows(Permission::UiAdd);
        let filename = manifest.entry.display().to_string();
        let loaded = context.with(|ctx| {
            let schedule = schedule_function(&ctx, &timers)?;
            let (listen, unlisten) = subscribe_functions(&ctx, &subscriptions)?;
            let check_place = Function::new(ctx.clone(), check_place)?;
            let factory: Function = caught(&ctx, ctx.eval(PRELUDE))?;
            let api: Object = caught(
                &ctx,
                factory.call((schedule, can_place, listen, unlisten, check_place)),
            )?;
            let fire: Function = caught(&ctx, api.get("fire"))?;
            let emit: Function = caught(&ctx, api.get("emit"))?;
            let instances: Function = caught(&ctx, api.get("instances"))?;
            let invoke: Function = caught(&ctx, api.get("invoke"))?;
            let render: Function = caught(&ctx, api.get("render"))?;

            let mut options = EvalOptions::default();
            options.filename = Some(filename);
            caught(&ctx, ctx.eval_with_options::<(), _>(script, options))?;

            Ok::<_, SandboxError>((
                Persistent::save(&ctx, fire),
                Persistent::save(&ctx, emit),
                Persistent::save(&ctx, instances),
                Persistent::save(&ctx, invoke),
                Persistent::save(&ctx, render),
            ))
        });
        let drained = drain_jobs(&runtime);
        budget.stop();
        let (fire, emit, instances, invoke, render) = loaded?;
        drained?;

        Ok(Self {
            fire,
            emit,
            instances,
            invoke,
            render,
            context,
            runtime,
            id: manifest.id,
            timers,
            subscriptions,
            budget,
            images: Images::new(root),
        })
    }

    /// When the earliest pending timer is due.
    pub(crate) fn next_timer(&self) -> Option<Instant> {
        locked(&self.timers)
            .pending
            .iter()
            .map(|(_, due)| *due)
            .min()
    }

    /// Runs every timer due at `now`, in due order. Timers they set land after `now`.
    pub(crate) fn fire_due(&mut self, now: Instant) -> Result<(), SandboxError> {
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
                let fire = self.fire.clone().restore(ctx)?;
                caught(ctx, fire.call::<_, ()>((id,)))
            })?;
        }

        Ok(())
    }

    /// Hands an update to the callbacks subscribed to its topic.
    pub(crate) fn deliver(&mut self, delivery: &Delivery) -> Result<(), SandboxError> {
        self.enter(|ctx| {
            let emit = self.emit.clone().restore(ctx)?;
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
            let set = self.instances.clone().restore(ctx)?;
            caught(ctx, set.call::<_, ()>((json,)))
        })
    }

    /// Runs the callback an event names. A callback from an older render, or one
    /// that takes another kind of value, is ignored.
    pub(crate) fn handle(&mut self, event: &UiEvent) -> Result<(), SandboxError> {
        let id = Into::<f64>::into(event.id);
        let argument = event.action.argument();

        self.enter(|ctx| {
            let invoke = self.invoke.clone().restore(ctx)?;
            caught(ctx, invoke.call::<_, bool>((id, argument)))
        })?;

        Ok(())
    }

    /// The placements to show, or `None` when nothing changed since the last call.
    pub(crate) fn render(&mut self) -> Result<Option<Vec<Placement>>, SandboxError> {
        let entries = self.enter(|ctx| {
            let render = self.render.clone().restore(ctx)?;
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
        for json in &entries {
            if json.len() > MAX_RENDER_BYTES {
                return Err(SandboxError::TooLarge(json.len()));
            }

            let raw: RawPlacement = serde_json::from_str(json)?;
            let slot = raw.place.slot();
            let mut nodes = 0;
            let node = self.images.resolve(raw.node, &mut nodes)?;
            if !slot.is_interactive() && node.is_interactive() {
                return Err(SandboxError::NotInteractive(slot));
            }

            placements.push(Placement {
                plugin: self.id.clone(),
                slot,
                instance: raw.instance,
                position: raw.place.position(),
                node,
            });
        }

        Ok(Some(placements))
    }

    /// Runs script code under the CPU budget, then drains promise jobs.
    fn enter<R>(
        &self,
        work: impl for<'js> FnOnce(&Ctx<'js>) -> Result<R, SandboxError>,
    ) -> Result<R, SandboxError> {
        self.budget.start();
        let result = self.context.with(|ctx| work(&ctx));
        let drained = drain_jobs(&self.runtime);
        self.budget.stop();

        let value = result?;
        drained?;

        Ok(value)
    }
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

pub(crate) fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Turns image paths into `data:` URLs, caching each path.
struct Images {
    root: PathBuf,
    cache: HashMap<String, String>,
}

impl Images {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            cache: HashMap::new(),
        }
    }

    fn resolve(&mut self, raw: RawNode, nodes: &mut usize) -> Result<Node, SandboxError> {
        *nodes += 1;
        if *nodes > MAX_NODES {
            return Err(SandboxError::TooManyNodes(*nodes));
        }

        Ok(match raw {
            RawNode::Image {
                src,
                width,
                height,
                pixelated,
            } => Node::Image {
                src: self
                    .data_url(&src)
                    .map_err(|reason| SandboxError::Image { src, reason })?,
                width,
                height,
                pixelated,
            },
            RawNode::Text { text, tone } => Node::Text { text, tone },
            RawNode::Row { gap, children } => Node::Row {
                gap,
                children: self.resolve_all(children, nodes)?,
            },
            RawNode::Button {
                label,
                children,
                on_press,
            } => Node::Button {
                label,
                children: self.resolve_all(children, nodes)?,
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
        nodes: &mut usize,
    ) -> Result<Vec<Node>, SandboxError> {
        children
            .into_iter()
            .map(|child| self.resolve(child, nodes))
            .collect()
    }

    fn data_url(&mut self, src: &str) -> Result<String, ImageError> {
        if let Some(url) = self.cache.get(src) {
            return Ok(url.clone());
        }
        if !Path::new(src).is_relative() {
            return Err(ImageError::Absolute);
        }

        let path = self.root.join(src).canonicalize()?;
        if !path.starts_with(&self.root) {
            return Err(ImageError::Outside);
        }
        let mime = mime_guess::from_path(&path)
            .first()
            .filter(|mime| mime.type_() == mime_guess::mime::IMAGE)
            .ok_or(ImageError::NotImage)?;
        let size = std::fs::metadata(&path)?.len();
        if size > MAX_IMAGE_BYTES {
            return Err(ImageError::TooLarge(size));
        }

        let bytes = std::fs::read(&path)?;
        let url = format!("data:{mime};base64,{}", STANDARD.encode(bytes));
        self.cache.insert(src.to_owned(), url.clone());

        Ok(url)
    }
}

#[cfg(test)]
#[path = "tests/sandbox.rs"]
mod tests;
