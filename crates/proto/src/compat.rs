//! Keeps the server readable by the apps already out there.
//!
//! `tests/wire/<wire>/` holds, for every request, the msgpack of sample
//! values of the request and of its reply, as clients reading that wire
//! revision send and read them. The test checks that each request fixture
//! still decodes, so a released app's requests keep working, and that the
//! reply samples still encode to the same bytes, so a released app can still
//! read what the server sends. Fixtures are append-only: run with
//! `BEEP_WIRE_BLESS=1` to write the ones a new request or a new `WIRE` lacks.
//! It never overwrites a fixture.

use crate::{link::LinkOffer, requests::each_request, *};
use iroh::SecretKey;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    env, fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

/// Values that cover a type's wire shapes: every variant of an enum, and
/// every sample of each field somewhere.
pub(crate) trait Sample: Sized {
    /// What a client reading `wire` may be sent; never empty.
    fn samples(wire: u32) -> Vec<Self>;
}

pub(crate) trait Visit {
    fn request<R: Rpc + Sample, T: Serialize + Sample>(&mut self);
}

/// The `k`th sample of `T`, wrapping around; sets `more` while `T` has
/// samples past `k`.
pub(crate) fn pick<T: Sample>(wire: u32, k: usize, more: &mut bool) -> T {
    let mut samples = T::samples(wire);
    assert!(!samples.is_empty(), "a type with no samples");
    if k + 1 < samples.len() {
        *more = true;
    }
    let i = k % samples.len();
    samples.swap_remove(i)
}

/// Pushes onto `out` values of the struct or variant `ctor` until each field
/// has taken every one of its samples.
macro_rules! sample_each {
    ($out:ident, $wire:ident, [$($ctor:tt)+] { $($field:ident),* }) => {{
        let _ = $wire;
        for k in 0usize.. {
            let _ = k;
            #[allow(unused_mut)]
            let mut more = false;
            $out.push($($ctor)+ { $($field: $crate::compat::pick($wire, k, &mut more)),* });
            if !more {
                break;
            }
        }
    }};
}

/// `Sample` for a plain struct, listing all its fields.
macro_rules! sample_struct {
    ($($name:ident { $($field:ident),* $(,)? })*) => {$(
        impl $crate::compat::Sample for $name {
            fn samples(wire: u32) -> Vec<Self> {
                let mut out = Vec::new();
                sample_each!(out, wire, [$name] { $($field),* });
                out
            }
        }
    )*};
}

macro_rules! sample_values {
    ($($ty:ty => [$($value:expr),+ $(,)?])*) => {$(
        impl Sample for $ty {
            fn samples(_: u32) -> Vec<Self> {
                vec![$($value),+]
            }
        }
    )*};
}

sample_values! {
    () => [()]
    bool => [true, false]
    u8 => [7]
    u32 => [42]
    u64 => [70_000]
    i32 => [-2]
    i64 => [1_760_000_000_000]
    f64 => [1.5]
    String => ["text".to_owned()]
    AgentSlug => [AgentSlug::from("text")]
    ModelId => [ModelId::from("text")]
    FlshModel => ["openrouter:vendor/model".parse().expect("a valid model")]
    FlshSort => [FlshSort::Price, FlshSort::Throughput, FlshSort::Latency]
    FlshQuantization => [
        FlshQuantization::Int4,
        FlshQuantization::Int8,
        FlshQuantization::Fp4,
        FlshQuantization::Fp6,
        FlshQuantization::Fp8,
        FlshQuantization::Fp16,
        FlshQuantization::Bf16,
        FlshQuantization::Fp32,
        FlshQuantization::Unknown,
    ]
    Uuid => [Uuid::from_u128(0x0123_4567_89ab_cdef_0123_4567_89ab_cdef)]
    SystemTime => [UNIX_EPOCH + Duration::from_millis(1_760_000_000_123)]
    ReasoningLevel => [
        ReasoningLevel::Off,
        ReasoningLevel::Low,
        ReasoningLevel::Medium,
        ReasoningLevel::High,
    ]
    ChartKind => [ChartKind::Line, ChartKind::Area, ChartKind::Bar]
    ChartAxis => [ChartAxis::Time, ChartAxis::Category]
    ChartRange => [
        ChartRange::Day,
        ChartRange::Week,
        ChartRange::Month,
        ChartRange::HalfYear,
        ChartRange::YearToDate,
        ChartRange::Year,
        ChartRange::FiveYears,
        ChartRange::Max,
    ]
}

impl<T: Sample> Sample for Option<T> {
    fn samples(wire: u32) -> Vec<Self> {
        let mut out: Vec<Self> = T::samples(wire).into_iter().map(Some).collect();
        out.push(None);
        out
    }
}

impl<T: Sample> Sample for Vec<T> {
    fn samples(wire: u32) -> Vec<Self> {
        vec![T::samples(wire)]
    }
}

impl Sample for SecretText {
    fn samples(_: u32) -> Vec<Self> {
        vec![SecretText::new("secret-1234")]
    }
}

impl Sample for LinkOffer {
    fn samples(_: u32) -> Vec<Self> {
        let device = SecretKey::from_bytes(&[3; 32]);
        let server = SecretKey::from_bytes(&[4; 32]).public();
        let at = UNIX_EPOCH + Duration::from_secs(1_760_000_000);
        vec![LinkOffer::sign(&device, server, "Laptop", at).expect("the name is valid")]
    }
}

const BLESS: &str = "BEEP_WIRE_BLESS";
const ERRORS: &str = "error.reply";

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/wire")
}

fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    rmp_serde::to_vec(value).expect("samples encode")
}

/// One wire revision's fixtures and what checking them found.
struct Revision {
    wire: u32,
    dir: PathBuf,
    files: BTreeMap<String, Vec<u8>>,
    used: BTreeSet<String>,
    bless: bool,
    written: Vec<String>,
    problems: Vec<String>,
}

impl Revision {
    fn load(wire: u32, dir: PathBuf, bless: bool) -> Self {
        let mut files = BTreeMap::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let bytes = fs::read(entry.path()).expect("fixture reads");
                files.insert(name, bytes);
            }
        }
        Self {
            wire,
            dir,
            files,
            used: BTreeSet::new(),
            bless,
            written: Vec::new(),
            problems: Vec::new(),
        }
    }

    /// The fixture for `op` ending in `.ext`, matched by number so a renamed
    /// request keeps its fixtures.
    fn find(&self, op: u16, ext: &str) -> Option<String> {
        let prefix = format!("{op:03}-");
        let suffix = format!(".{ext}");
        self.files
            .keys()
            .find(|name| name.starts_with(&prefix) && name.ends_with(&suffix))
            .cloned()
    }

    fn write(&mut self, name: String, bytes: Vec<u8>) {
        fs::create_dir_all(&self.dir).expect("fixture dir");
        let path = self.dir.join(&name);
        assert!(!path.exists(), "{} exists", path.display());
        fs::write(&path, &bytes).expect("fixture writes");
        self.files.insert(name.clone(), bytes);
        self.used.insert(name.clone());
        self.written.push(name);
    }

    /// Checks the reply `name` against what the server sends now.
    fn check_reply(&mut self, name: &str, now: Vec<u8>) {
        self.used.insert(name.to_owned());
        if self.files[name] != now {
            self.problems.push(format!(
                "{}/{name}: the server's reply changed shape, and apps reading wire {} \
                 cannot decode it. Put the new shape behind a new request, or behind a \
                 `Feature` and `since` (see AGENTS.md).",
                self.wire, self.wire
            ));
        }
    }

    fn check_errors(&mut self) {
        let now = encode(&ApiError::samples(self.wire));
        if self.files.contains_key(ERRORS) {
            self.check_reply(ERRORS, now);
        } else if self.bless && self.wire == WIRE {
            self.write(ERRORS.to_owned(), now);
        } else {
            self.problems
                .push(format!("{}/{ERRORS} is missing", self.wire));
        }
    }

    fn check_leftovers(&mut self) {
        for name in self.files.keys().filter(|name| !self.used.contains(*name)) {
            self.problems.push(format!(
                "{}/{name} matches no request: requests are never removed and their \
                 numbers never reused",
                self.wire
            ));
        }
    }
}

impl Visit for Revision {
    fn request<R: Rpc + Sample, T: Serialize + Sample>(&mut self) {
        let op = R::OP as u16;
        let request = self.find(op, "request");
        let reply = self.find(op, "reply");
        let (request, reply) = match (request, reply) {
            (Some(request), Some(reply)) => (request, reply),
            // Newer than this revision, so none of its clients send it.
            (None, None) if self.wire < WIRE => return,
            (None, None) if self.bless => {
                let stem = format!("{op:03}-{:?}", R::OP);
                self.write(format!("{stem}.request"), encode(&R::samples(self.wire)));
                self.write(format!("{stem}.reply"), encode(&T::samples(self.wire)));
                return;
            }
            (None, None) => {
                self.problems.push(format!(
                    "{:?} has no fixtures in {}/: run `{BLESS}=1 cargo test -p beep-proto \
                     compat` and commit them",
                    R::OP,
                    self.wire
                ));
                return;
            }
            _ => {
                self.problems
                    .push(format!("{:?} lacks a request or reply fixture", R::OP));
                return;
            }
        };
        self.used.insert(request.clone());
        if let Err(e) = rmp_serde::from_slice::<Vec<R>>(&self.files[&request]) {
            self.problems.push(format!(
                "{}/{request}: apps reading wire {} send this, and it no longer decodes \
                 ({e}). Requests only gain trailing `#[serde(default)]` fields.",
                self.wire, self.wire
            ));
        }
        self.check_reply(&reply, encode(&T::samples(self.wire)));
    }
}

#[test]
fn released_apps_still_read_the_wire() {
    let bless = env::var_os(BLESS).is_some();
    let root = fixtures_dir();
    let mut wires: BTreeSet<u32> = fs::read_dir(&root)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().to_str()?.parse().ok())
        .collect();
    let mut problems = Vec::new();
    if !wires.contains(&WIRE) {
        if bless {
            wires.insert(WIRE);
        } else {
            problems.push(format!(
                "tests/wire/{WIRE} is missing: run `{BLESS}=1 cargo test -p beep-proto compat`"
            ));
        }
    }
    for &wire in &wires {
        if wire > WIRE {
            problems.push(format!("tests/wire/{wire} is newer than WIRE ({WIRE})"));
            continue;
        }
        let mut revision = Revision::load(wire, root.join(wire.to_string()), bless);
        each_request(&mut revision);
        revision.check_errors();
        revision.check_leftovers();
        for name in &revision.written {
            println!("wrote tests/wire/{wire}/{name}");
        }
        problems.append(&mut revision.problems);
    }
    assert!(problems.is_empty(), "\n{}\n", problems.join("\n"));
}

#[test]
fn old_apps_refuse_an_extra_element() {
    #[derive(Serialize)]
    struct Newer {
        a: u8,
        b: u8,
    }
    #[derive(serde::Deserialize, Debug)]
    struct Older {
        _a: u8,
    }
    let newer = encode(&Newer { a: 1, b: 2 });
    assert!(rmp_serde::from_slice::<Older>(&newer).is_err());
    // Inside a wire enum too: `[op, message, extra]`.
    let extra = encode(&(0u8, "x", true));
    assert!(rmp_serde::from_slice::<ApiError>(&extra).is_err());
}

#[test]
fn old_apps_refuse_an_unknown_op() {
    assert!(rmp_serde::from_slice::<ApiError>(&encode(&(9u8,))).is_err());
}
