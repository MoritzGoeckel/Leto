use super::super::Loop;
use crate::ui::AskOptions;
use ratatui::style::Color;

pub(super) fn run(loop_state: &mut Loop) -> Result<(), Box<dyn std::error::Error>> {
    let levels = ["off", "minimal", "low", "medium", "high", "xhigh", "max"];
    let values = [
        crate::core::ThinkingLevel::Off,
        crate::core::ThinkingLevel::Minimal,
        crate::core::ThinkingLevel::Low,
        crate::core::ThinkingLevel::Medium,
        crate::core::ThinkingLevel::High,
        crate::core::ThinkingLevel::Xhigh,
        crate::core::ThinkingLevel::Max,
    ];
    let mut ui = loop_state.ui.lock().unwrap();
    ui.append_message_str(&levels.join("\n"));
    let answer = ui.ask_styled(AskOptions {
        background: Color::Rgb(45, 39, 26),
        form_text: "Reasoning level...".to_owned(),
        ..AskOptions::default()
    })?;
    if let Some((index, _)) = levels
        .iter()
        .enumerate()
        .find(|(_, level)| **level == answer)
    {
        loop_state.reasoning = Some(values[index].clone());
        loop_state.config.set_model(crate::config::ModelConfig {
            provider: loop_state.model.provider.clone(),
            model: loop_state.model.id.clone(),
            reasoning: loop_state.reasoning.clone(),
        });
        loop_state.config.save()?;
        ui.append_message_str(&format!("Reasoning has been changed to {answer}"));
    } else {
        ui.append_message_str("Reasoning level not found");
    }
    Ok(())
}
