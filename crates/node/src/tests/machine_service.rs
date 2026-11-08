use super::*;

pub(super) fn launch(workspace: Option<&str>) -> Launch {
    Launch {
        exe: PathBuf::from("/opt/beep node/beep-node"),
        dir: PathBuf::from("/srv/node"),
        workspace: workspace.map(PathBuf::from),
        search: "/usr/bin:/home/user/50%/bin".to_owned(),
    }
}

fn linux(
    user: bool,
    root: bool,
    container: bool,
    init: Option<Manager>,
) -> Result<Manager, Unusable> {
    let user = if user {
        Ok(())
    } else {
        Err("no bus".to_owned())
    };
    choose("linux", || Linux {
        user,
        root,
        container,
        init,
    })
}

#[test]
fn a_user_instance_comes_first() {
    let init = Some(Manager::SystemdSystem);
    assert_eq!(linux(true, false, false, init), Ok(Manager::Systemd));
    assert_eq!(linux(true, true, false, init), Ok(Manager::Systemd));
}

#[test]
fn root_without_a_user_instance_uses_the_init() {
    for init in [Manager::SystemdSystem, Manager::OpenRc, Manager::SysVInit] {
        assert_eq!(linux(false, true, false, Some(init)), Ok(init));
    }
}

#[test]
fn others_are_told_to_be_root_or_run_it_themselves() {
    let e = linux(false, false, false, Some(Manager::OpenRc)).unwrap_err();
    assert_eq!(
        e,
        Unusable::NotRoot {
            init: Manager::OpenRc,
            why: "no bus".to_owned()
        }
    );
    let said = e.to_string();
    assert!(said.contains("runs OpenRC: rerun as root"), "{said}");
    assert!(said.contains("`beep-node run`"), "{said}");
    assert_eq!(
        linux(false, true, false, None),
        Err(Unusable::NoInit("no bus".to_owned()))
    );
}

#[test]
fn a_container_without_an_init_is_its_own_service() {
    assert_eq!(linux(false, true, true, None), Err(Unusable::Container));
    assert_eq!(linux(false, false, true, None), Err(Unusable::Container));
    let said = Unusable::Container.to_string();
    assert!(said.contains("--restart unless-stopped"), "{said}");
    // One that runs an init is a machine like any other.
    assert_eq!(
        linux(false, true, true, Some(Manager::OpenRc)),
        Ok(Manager::OpenRc)
    );
}

#[test]
fn the_init_is_what_runs_not_what_is_installed() {
    let on = |paths: &'static [&'static str]| move |path: &str| paths.contains(&path);
    // Debian on systemd keeps /etc/init.d.
    assert_eq!(
        running_init(false, on(&["/run/systemd/system", "/etc/init.d"])),
        Some(Manager::SystemdSystem)
    );
    assert_eq!(
        running_init(false, on(&["/sbin/openrc-run", "/etc/init.d"])),
        Some(Manager::OpenRc)
    );
    assert_eq!(
        running_init(false, on(&["/etc/init.d"])),
        Some(Manager::SysVInit)
    );
    assert_eq!(running_init(false, on(&[])), None);
    // A container image holds these files with no init started.
    assert_eq!(
        running_init(true, on(&["/sbin/openrc-run", "/etc/init.d"])),
        None
    );
    assert_eq!(
        running_init(true, on(&["/run/openrc", "/etc/init.d"])),
        Some(Manager::OpenRc)
    );
    assert_eq!(
        running_init(true, on(&["/run/systemd/system"])),
        Some(Manager::SystemdSystem)
    );
}

#[test]
fn containers_are_known_by_marker_or_by_pid_1() {
    let none = |_: &str| false;
    assert!(!in_container(none, None));
    assert!(!in_container(none, Some(b"PATH=/bin\0HOME=/\0")));
    assert!(in_container(none, Some(b"PATH=/bin\0container=lxc\0")));
    assert!(in_container(|path| path == "/.dockerenv", None));
    assert!(in_container(|path| path == "/run/.containerenv", None));
}
