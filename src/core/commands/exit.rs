use super::super::Loop;

pub(super) fn run(loop_state: &mut Loop) -> Result<(), Box<dyn std::error::Error>> {
    loop_state.exit = true;
    Ok(())
}
