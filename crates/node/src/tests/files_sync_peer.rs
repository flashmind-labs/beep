use super::*;

/// `Reply::Head` as peers from before `behind` have it.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
enum OldReply {
    Head { commit: Option<Oid> },
}

#[test]
fn head_stays_readable_both_ways_within_the_alpn() {
    let new = rmp_serde::to_vec(&Reply::Head {
        commit: None,
        behind: Some(7),
        has_git: true,
        branch: None,
    })
    .unwrap();
    assert_eq!(
        beep_proto::lenient::from_slice::<OldReply>(&new).unwrap(),
        OldReply::Head { commit: None }
    );
    let old = rmp_serde::to_vec(&OldReply::Head { commit: None }).unwrap();
    assert_eq!(
        beep_proto::lenient::from_slice::<Reply>(&old).unwrap(),
        Reply::Head {
            commit: None,
            behind: None,
            has_git: false,
            branch: None,
        }
    );
}
