use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use rquickjs::context::EvalOptions;
use rquickjs::{CatchResultExt, Context, Ctx, Function, Object, Persistent, Runtime};
use thiserror::Error;

use crate::manifest::{Manifest, ManifestError, Permission, PluginId};
use crate::source::{SourceError, strip_types};
use crate::ui::{Node, Placement, RawNode, RawPlacement};

const PRELUDE: &str = include_str!("prelude.js");

const MEMORY_LIMIT: usize = 16 * 1024 * 1024;
const STACK_LIMIT: usize = 256 * 1024;
const CPU_BUDGET: Duration = Duration::from_millis(50);
const MAX_TIMERS: usize = 64;
const MIN_TIMER_MS: f64 = 16.0;
const MAX_TIMER_MS: f64 = 86_400_000.0;
const MAX_PLACEMENTS: usize = 8;
const MAX_NODES: usize = 64;
const MAX_RENDER_BYTES: usize = 64 * 1024;
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
    #[error("placed {0} things, the limit is {MAX_PLACEMENTS}")]
    TooManyPlacements(usize),
    #[error("rendered {0} bytes, the limit is {MAX_RENDER_BYTES}")]
    TooLarge(usize),
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
    render: Persistent<Function<'static>>,
    context: Context,
    runtime: Runtime,
    id: PluginId,
    timers: Arc<Mutex<Timers>>,
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

        budget.start();
        let can_place = manifest.allows(Permission::UiAdd);
        let filename = manifest.entry.display().to_string();
        let loaded = context.with(|ctx| {
            let schedule = schedule_function(&ctx, &timers)?;
            let factory: Function = caught(&ctx, ctx.eval(PRELUDE))?;
            let api: Object = caught(&ctx, factory.call((schedule, can_place)))?;
            let fire: Function = caught(&ctx, api.get("fire"))?;
            let render: Function = caught(&ctx, api.get("render"))?;

            let mut options = EvalOptions::default();
            options.filename = Some(filename);
            caught(&ctx, ctx.eval_with_options::<(), _>(script, options))?;

            Ok::<_, SandboxError>((Persistent::save(&ctx, fire), Persistent::save(&ctx, render)))
        });
        let drained = drain_jobs(&runtime);
        budget.stop();
        let (fire, render) = loaded?;
        drained?;

        Ok(Self {
            fire,
            render,
            context,
            runtime,
            id: manifest.id,
            timers,
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

    /// The placements to show, or `None` when nothing changed since the last call.
    pub(crate) fn render(&mut self) -> Result<Option<Vec<Placement>>, SandboxError> {
        let json = self.enter(|ctx| {
            let render = self.render.clone().restore(ctx)?;
            caught(ctx, render.call::<_, Option<String>>(()))
        })?;
        let Some(json) = json else {
            return Ok(None);
        };

        if json.len() > MAX_RENDER_BYTES {
            return Err(SandboxError::TooLarge(json.len()));
        }
        let raw: Vec<RawPlacement> = serde_json::from_str(&json)?;
        if raw.len() > MAX_PLACEMENTS {
            return Err(SandboxError::TooManyPlacements(raw.len()));
        }

        let mut nodes = 0;
        let mut placements = Vec::with_capacity(raw.len());
        for item in raw {
            placements.push(Placement {
                plugin: self.id.clone(),
                slot: item.slot,
                align: item.align,
                offset: item.offset,
                sink: item.sink,
                node: self.images.resolve(item.node, &mut nodes)?,
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
            RawNode::Text { text } => Node::Text { text },
            RawNode::Row { gap, children } => Node::Row {
                gap,
                children: children
                    .into_iter()
                    .map(|child| self.resolve(child, nodes))
                    .collect::<Result<_, _>>()?,
            },
        })
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
