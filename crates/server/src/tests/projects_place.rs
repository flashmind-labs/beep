use super::*;
use iroh::SecretKey;

fn fit(n: u8, name: &str, capabilities: &[Capability]) -> MachineFit {
    MachineFit {
        machine: SecretKey::from_bytes(&[n; 32]).public(),
        name: name.to_owned(),
        online: true,
        offers_agent: true,
        checkout: Some(
            ("/srv/project")
                .parse::<MachinePath>()
                .expect("a valid path"),
        ),
        shared: true,
        capabilities: capabilities.to_vec(),
        running: 0,
    }
}

fn any() -> PlaceRequest<'static> {
    PlaceRequest {
        named: None,
        pin: None,
        needs: &[],
    }
}

#[test]
fn named_machine_wins() {
    let fits = [fit(1, "build-box", &[]), fit(2, "gpu-box", &[])];
    let request = PlaceRequest {
        named: Some("GPU box"),
        ..any()
    };
    assert_eq!(place(&request, &fits), Ok(1));
}

#[test]
fn pin_beats_capabilities() {
    let fits = [
        fit(1, "build-box", &[]),
        fit(2, "gpu-box", &[Capability::Cuda]),
    ];
    let request = PlaceRequest {
        pin: Some(&fits[0].machine),
        needs: &[Capability::Cuda],
        ..any()
    };
    assert_eq!(place(&request, &fits), Ok(0));
}

#[test]
fn capabilities_pick_gpu_box() {
    let fits = [
        fit(1, "build-box", &[]),
        fit(2, "gpu-box", &[Capability::Cuda]),
    ];
    let request = PlaceRequest {
        needs: &[Capability::Cuda],
        ..any()
    };
    assert_eq!(place(&request, &fits), Ok(1));
    assert_eq!(
        place(
            &PlaceRequest {
                needs: &[Capability::Xcode],
                ..any()
            },
            &fits
        ),
        Err(PlaceError::NoneCovers(vec![Capability::Xcode]))
    );
}

#[test]
fn unshared_checkout_is_skipped() {
    let mut first = fit(1, "build-box", &[]);
    first.shared = false;
    let fits = [first, fit(2, "gpu-box", &[])];
    assert_eq!(place(&any(), &fits), Ok(1));
}

#[test]
fn fewest_running_breaks_ties() {
    let mut busy = fit(1, "a-box", &[]);
    busy.running = 1;
    let fits = [busy, fit(2, "c-box", &[]), fit(3, "b-box", &[])];
    assert_eq!(place(&any(), &fits), Ok(2));
}

#[test]
fn full_machines_are_skipped() {
    let mut full = fit(1, "a-box", &[]);
    full.running = 2;
    let mut fits = [full, fit(2, "b-box", &[])];
    assert_eq!(place(&any(), &fits), Ok(1));
    fits[1].running = 2;
    assert_eq!(place(&any(), &fits), Err(PlaceError::NoneFree));
}
