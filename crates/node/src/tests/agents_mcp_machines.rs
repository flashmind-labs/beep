use super::*;

#[test]
fn a_job_id_names_its_machine() {
    let named = name_the_job(
        "Started it in the background. Job id: 7. Check it.",
        "box-b",
    );
    assert_eq!(
        named,
        "Started it in the background. Job id: task:7@box-b. Check it."
    );
    let job = "task:7@box-b".parse::<Job>().expect("a job id");
    assert_eq!((job.id, job.machine.as_str()), (7, "box-b"));
    assert!("7".parse::<Job>().ok().is_none());
}

#[test]
fn a_machine_is_named_in_one_word() {
    assert_eq!(command_name(" Mac Studio "), "mac-studio");
    assert_eq!(command_name("Alex’s Mac Studio"), "alexs-mac-studio");
    assert_eq!(command_name("Alex's  MacBook (2)"), "alexs-macbook-2");
}
