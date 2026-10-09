use super::super::Loop;
use crate::core::Context;
use crate::core::events::EventValue;

pub(super) fn run(loop_state: &mut Loop) -> Result<(), Box<dyn std::error::Error>> {
    let agents_md = Loop::load_agents_md();
    loop_state.context = Context {
        system_prompt: agents_md.map(|(_, content)| content),
        tools: Some(
            loop_state
                .tools
                .tools
                .values()
                .map(|tool| tool.definition.clone())
                .collect(),
        ),
        ..Context::default()
    };
    if let Some(system_prompt) = &loop_state.context.system_prompt {
        loop_state
            .events
            .append(EventValue::SystemPrompt(system_prompt.clone()), false)?;
    }
    Ok(())
}
