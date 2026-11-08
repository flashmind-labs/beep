use super::*;

fn candidate<'a>(id: &'a str, online: bool, shell: bool, workspace: bool) -> Candidate<'a> {
    Candidate {
        id,
        name: id,
        online,
        shell,
        workspace,
    }
}

#[test]
fn auto_prefers_the_environment_then_the_only_machine() {
    let machines = [
        candidate("mac", true, true, false),
        candidate("box", true, true, true),
    ];
    assert_eq!(choose(&machines, None), Ok(1));
    assert_eq!(choose(&machines, Some(" ")), Ok(1));
    let one = [
        candidate("box", false, false, true),
        candidate("mac", true, true, false),
    ];
    assert_eq!(choose(&one, None), Ok(1));
    let two = [
        candidate("mac", true, true, false),
        candidate("studio", true, true, false),
    ];
    assert_eq!(
        choose(&two, None),
        Err(ImportError::PickOne("mac, studio".into()))
    );
    assert_eq!(
        choose(&[candidate("box", false, false, true)], None),
        Err(ImportError::NoneOnline)
    );
}
