use super::*;

const HERE: Presence = Presence {
    idle: Duration::from_secs(5),
    locked: false,
    mic_busy: false,
};

#[test]
fn speaks_only_to_someone_here_and_free() {
    assert!(HERE.can_speak());
    assert!(
        !Presence {
            idle: Duration::from_secs(600),
            ..HERE
        }
        .can_speak()
    );
    assert!(
        !Presence {
            locked: true,
            ..HERE
        }
        .can_speak()
    );
    assert!(
        !Presence {
            mic_busy: true,
            ..HERE
        }
        .can_speak()
    );
}
