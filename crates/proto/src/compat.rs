//! Keeps the server reading what the apps already out there send.
//!
//! `tests/wire/named/` holds, for every request, the msgpack (with field
//! names, as the apps send it) of sample values of the request. The test
//! checks that each still decodes, so a request only gains fields that
//! default. Replies are not frozen: a reader skips fields it does not know,
//! so a reply only must not lose or rename a field. Fixtures are
//! append-only: run with `BEEP_WIRE_BLESS=1` to write the ones a new request
//! lacks. It never overwrites a fixture. The numbered folders are the
//! positional wire the apps spoke over iroh, kept as history and not read.

use crate::{http, link::LinkOffer, requests::each_request, *};
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
    /// Values covering the type's shapes; never empty.
    fn samples() -> Vec<Self>;
}

pub(crate) trait Visit {
    fn request<R: Rpc + Sample, T: Serialize + Sample>(&mut self);
}

/// The `k`th sample of `T`, wrapping around; sets `more` while `T` has
/// samples past `k`.
pub(crate) fn pick<T: Sample>(k: usize, more: &mut bool) -> T {
    let mut samples = T::samples();
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
    ($out:ident, [$($ctor:tt)+] { $($field:ident),* }) => {{
        for k in 0usize.. {
            let _ = k;
            #[allow(unused_mut)]
            let mut more = false;
            $out.push($($ctor)+ { $($field: $crate::compat::pick(k, &mut more)),* });
            if !more {
                break;
            }
        }
    }};
}

/// `Sample` for a plain struct, listing all its fields.
macro_rules! sample_struct {
    ($name:ident { $($field:ident),* $(,)? }) => {
        impl $crate::compat::Sample for $name {
            fn samples() -> Vec<Self> {
                let mut out = Vec::new();
                sample_each!(out, [$name] { $($field),* });
                out
            }
        }
    };
}

macro_rules! sample_values {
    ($($ty:ty => [$($value:expr),+ $(,)?])*) => {$(
        impl Sample for $ty {
            fn samples() -> Vec<Self> {
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
    serde_bytes::ByteBuf => [serde_bytes::ByteBuf::from(b"$ ls\r\n\x1b[32mok\x1b[0m".to_vec())]
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
    fn samples() -> Vec<Self> {
        let mut out: Vec<Self> = T::samples().into_iter().map(Some).collect();
        out.push(None);
        out
    }
}

impl<T: Sample> Sample for Vec<T> {
    fn samples() -> Vec<Self> {
        vec![T::samples()]
    }
}

impl<T: Sample> Sample for Box<T> {
    fn samples() -> Vec<Self> {
        T::samples().into_iter().map(Box::new).collect()
    }
}

impl Sample for SecretText {
    fn samples() -> Vec<Self> {
        vec![SecretText::new("secret-1234")]
    }
}

impl Sample for LinkOffer {
    fn samples() -> Vec<Self> {
        let device = SecretKey::from_bytes(&[3; 32]);
        let server = SecretKey::from_bytes(&[4; 32]).public();
        let at = UNIX_EPOCH + Duration::from_secs(1_760_000_000);
        vec![LinkOffer::sign(&device, server, "Laptop", at).expect("the name is valid")]
    }
}

const BLESS: &str = "BEEP_WIRE_BLESS";

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/wire/named")
}

fn encode<T: Serialize>(value: &T) -> Vec<u8> {
    http::encode(value).expect("samples encode")
}

/// The fixtures and what checking them found.
struct Fixtures {
    dir: PathBuf,
    files: BTreeMap<String, Vec<u8>>,
    used: BTreeSet<String>,
    bless: bool,
    written: Vec<String>,
    problems: Vec<String>,
}

impl Fixtures {
    fn load(dir: PathBuf, bless: bool) -> Self {
        let mut files = BTreeMap::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let bytes = fs::read(entry.path()).expect("fixture reads");
                files.insert(name, bytes);
            }
        }
        Self {
            dir,
            files,
            used: BTreeSet::new(),
            bless,
            written: Vec::new(),
            problems: Vec::new(),
        }
    }

    /// The fixture for `op`, matched by number so a renamed request keeps it.
    fn find(&self, op: u16) -> Option<String> {
        let prefix = format!("{op:03}-");
        self.files
            .keys()
            .find(|name| name.starts_with(&prefix) && name.ends_with(".request"))
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

    fn check_leftovers(&mut self) {
        for name in self.files.keys().filter(|name| !self.used.contains(*name)) {
            self.problems.push(format!(
                "named/{name} matches no request: released apps still send it, so keep \
                 serving it under its name"
            ));
        }
    }
}

impl Visit for Fixtures {
    fn request<R: Rpc + Sample, T: Serialize + Sample>(&mut self) {
        let op = R::OP as u16;
        let Some(name) = self.find(op) else {
            if self.bless {
                self.write(
                    format!("{op:03}-{:?}.request", R::OP),
                    encode(&R::samples()),
                );
            } else {
                self.problems.push(format!(
                    "{:?} has no fixture in named/: run `{BLESS}=1 cargo test -p beep-proto \
                     compat` and commit it",
                    R::OP
                ));
            }
            return;
        };
        self.used.insert(name.clone());
        if let Err(e) = http::decode::<Vec<R>>(&self.files[&name]) {
            self.problems.push(format!(
                "named/{name}: released apps send this, and it no longer decodes ({e}). \
                 A request only gains fields that default."
            ));
        }
    }
}

#[test]
fn released_apps_requests_still_decode() {
    let bless = env::var_os(BLESS).is_some();
    let mut fixtures = Fixtures::load(fixtures_dir(), bless);
    each_request(&mut fixtures);
    fixtures.check_leftovers();
    for name in &fixtures.written {
        println!("wrote tests/wire/named/{name}");
    }
    assert!(
        fixtures.problems.is_empty(),
        "\n{}\n",
        fixtures.problems.join("\n")
    );
}

#[test]
fn a_reader_skips_a_field_it_does_not_know() {
    #[derive(Serialize)]
    struct Newer {
        a: u8,
        b: u8,
    }
    #[derive(serde::Deserialize, Debug, PartialEq)]
    struct Older {
        a: u8,
    }
    let newer = encode(&Newer { a: 1, b: 2 });
    assert_eq!(http::decode::<Older>(&newer).unwrap(), Older { a: 1 });
}
