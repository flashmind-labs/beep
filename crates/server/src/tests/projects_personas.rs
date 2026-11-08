use super::*;

#[test]
fn persona_reads_beep_keys() {
    let text = "---\nname: blender-rigger\ndescription: Rigs characters\nengine: claude\n\
                    needs: [cuda, blender]\nmcp: [blender]\n---\nYou rig models.\n";
    let found = parse(text).unwrap();
    assert_eq!(found.engine, Some(WorkerAgent::Claude));
    assert_eq!(found.needs, [Capability::Cuda, Capability::Blender]);
    assert_eq!(found.persona.name, "blender-rigger");
    assert_eq!(found.persona.system_prompt, "You rig models.");
    assert!(found.persona.notices.is_empty());
}
