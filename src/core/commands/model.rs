use super::super::Loop;
use crate::provider::Provider;
use crate::ui::AskOptions;
use ratatui::style::Color;

pub(super) fn run(loop_state: &mut Loop) -> Result<(), Box<dyn std::error::Error>> {
    let models = loop_state
        .provider
        .get_models()
        .into_values()
        .collect::<Vec<_>>();
    let names = models
        .iter()
        .map(|model| model.name.as_str())
        .collect::<Vec<_>>();
    let mut ui = loop_state.ui.lock().unwrap();
    ui.append_message_str(&names.join("\n"));
    let answer = ui.ask_styled(AskOptions {
        background: Color::Rgb(45, 39, 26),
        form_text: "Model name...".to_owned(),
        ..AskOptions::default()
    })?;
    if let Some(model) = models.into_iter().find(|model| model.name == answer) {
        loop_state.model = model;
        loop_state.config.set_model(crate::config::ModelConfig {
            provider: loop_state.model.provider.clone(),
            model: loop_state.model.id.clone(),
            reasoning: loop_state.reasoning.clone(),
        });
        loop_state.config.save()?;
        ui.append_message_str(&format!(
            "Model has been changed to {}",
            loop_state.model.name
        ));
    } else {
        ui.append_message_str("Model not found");
    }
    Ok(())
}
