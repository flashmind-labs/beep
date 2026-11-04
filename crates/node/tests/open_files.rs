//! The node raises its soft limit on open files at startup.
#![cfg(unix)]

use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};

#[test]
fn a_low_soft_limit_is_raised_within_the_hard_limit() {
    let hard = getrlimit(Resource::Nofile).maximum;
    setrlimit(
        Resource::Nofile,
        Rlimit {
            current: Some(256),
            maximum: hard,
        },
    )
    .expect("lowering the soft limit");

    beep_node::machine::raise_open_files();

    let now = getrlimit(Resource::Nofile);
    assert_eq!(now.maximum, hard);
    assert_eq!(
        now.current,
        Some(hard.map_or(10_240, |hard| hard.min(10_240)))
    );
}
