use super::{FILES_KEPT, LOG_FILTER, appender};
use std::{
    env, fs,
    io::Write,
    path::{Path, PathBuf},
};
use uuid::Uuid;

fn files(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(dir)
        .expect("read log directory")
        .map(|entry| entry.expect("log entry").path())
        .collect();
    files.sort();
    files
}

#[test]
fn daily_appender_uses_dated_files_and_keeps_three() {
    let dir = env::temp_dir().join(format!("beep-log-{}", Uuid::new_v4()));
    fs::create_dir_all(&dir).expect("create log directory");

    for day in 1..=4 {
        fs::write(dir.join(format!("beep.2000-01-{day:02}.log")), "old\n").expect("old log");
    }

    let mut writer = appender(&dir).expect("create appender");
    writer.write_all(b"today\n").expect("write log");
    writer.flush().expect("flush log");
    let kept = files(&dir);
    assert_eq!(kept.len(), FILES_KEPT);
    assert_eq!(
        fs::read_to_string(&kept[2]).expect("current log"),
        "today\n"
    );
    drop(writer);
    fs::remove_dir_all(&dir).expect("remove log directory");
}

#[test]
fn daily_appender_keeps_large_logs_and_appends_after_restart() {
    let dir = env::temp_dir().join(format!("beep-log-{}", Uuid::new_v4()));
    let mut writer = appender(&dir).expect("create appender");
    let data = vec![b'x'; 3 * 1024 * 1024];
    writer.write_all(&data).expect("write log");
    writer.write_all(&data).expect("write log");
    drop(writer);
    let mut writer = appender(&dir).expect("reopen appender");
    writer.write_all(b"restarted\n").expect("append log");
    writer.flush().expect("flush log");
    let kept = files(&dir);
    assert_eq!(kept.len(), 1);
    assert_eq!(
        fs::metadata(&kept[0]).expect("log metadata").len(),
        (2 * data.len() + 10) as u64
    );
    drop(writer);
    fs::remove_dir_all(&dir).expect("remove log directory");
}

#[test]
fn the_filter_parses_and_quiets_per_snapshot_lines() {
    let filter = tracing_subscriber::EnvFilter::try_new(LOG_FILTER).expect("log filter");
    let text = filter.to_string();
    assert!(text.contains("beep_desktop_lib::fleet::events=info"));
    assert!(text.contains("beep_node::link::events=warn"));
}
