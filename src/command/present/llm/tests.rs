use super::render::commands;

#[test]
fn command_spans_stop_before_prose_and_find_later_commands() {
    let instruction = concat!(
        "Run agentbus drain until remaining=0. ",
        "Start ~/Projects/example/legal.sh start <ticket>, then inspect with ",
        "rtk herdr agent read <pane_id> --lines 50."
    );

    assert_eq!(
        commands(instruction),
        concat!(
            "Run `agentbus drain` until remaining=0. ",
            "Start `~/Projects/example/legal.sh start <ticket>`, then inspect with ",
            "`rtk herdr agent read <pane_id> --lines 50`."
        )
    );
}

#[test]
fn sentence_boundaries_and_non_executable_paths_stay_prose() {
    let instruction = concat!(
        "rtk herdr pane close <pane_id>. If invalid: reopen with ",
        "~/Projects/example/legal.sh start <ticket>. ",
        "ntk ls is scoped by cwd. ",
        "~/Projects/example/repository — it is a bare clone."
    );

    assert_eq!(
        commands(instruction),
        concat!(
            "`rtk herdr pane close <pane_id>`. If invalid: reopen with ",
            "`~/Projects/example/legal.sh start <ticket>`. ",
            "`ntk ls` is scoped by cwd. ",
            "~/Projects/example/repository — it is a bare clone."
        )
    );
    assert_eq!(
        commands("Keep `ntk ls`; then ntk start <ticket>."),
        "Keep `ntk ls`; then `ntk start <ticket>`."
    );
    assert_eq!(
        commands("Open <project-root>/lane.sh start <ticket>, then report."),
        "Open `<project-root>/lane.sh start <ticket>`, then report."
    );
}
