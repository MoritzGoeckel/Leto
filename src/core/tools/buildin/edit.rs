use crate::core::Tool;
use crate::core::tools::ExecutableTool;
use serde_json::{Value, json};
use unicode_normalization::UnicodeNormalization;

pub(super) fn make_edit_tool() -> ExecutableTool {
    ExecutableTool {
        definition: Tool {
            name: "edit".to_string(),
            description:
                "Replace unique text in a file. Each oldText must match one non-overlapping region."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Path to the file to edit." },
                    "edits": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "oldText": { "type": "string", "description": "Unique text to replace." },
                                "newText": { "type": "string", "description": "Replacement text." }
                            },
                            "required": ["oldText", "newText"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["file", "edits"],
                "additionalProperties": false
            }),
            constrained_sampling: None,
        },
        handler: Box::new(|_, parameters: Value| {
            let file = parameters["file"].as_str().unwrap();
            let edits = parameters["edits"].as_array().unwrap();
            if edits.is_empty() {
                return Err(
                    "Edit tool input is invalid. edits must contain at least one replacement."
                        .to_string(),
                );
            }
            let original = std::fs::read_to_string(file).map_err(|error| error.to_string())?;
            let line_ending = if original.contains("\r\n") {
                "\r\n"
            } else {
                "\n"
            };
            let original = original.replace("\r\n", "\n").replace('\r', "\n");
            let edits: Vec<_> = edits
                .iter()
                .map(|edit| {
                    Ok((
                        edit["oldText"]
                            .as_str()
                            .unwrap()
                            .replace("\r\n", "\n")
                            .replace('\r', "\n"),
                        edit["newText"]
                            .as_str()
                            .unwrap()
                            .replace("\r\n", "\n")
                            .replace('\r', "\n"),
                    ))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let (content, replacements) = prepare_edits(&original, &edits, file)?;
            let mut content = content;
            for (start, end, replacement) in replacements.into_iter().rev() {
                content.replace_range(start..end, &replacement);
            }
            if line_ending == "\r\n" {
                content = content.replace('\n', "\r\n");
            }
            std::fs::write(file, content).map_err(|error| error.to_string())?;
            Ok(json!(format!(
                "Successfully replaced {} block(s) in {file}.",
                edits.len()
            )))
        }),
    }
}

fn prepare_edits(
    content: &str,
    edits: &[(String, String)],
    file: &str,
) -> Result<(String, Vec<(usize, usize, String)>), String> {
    if edits.iter().any(|(old_text, _)| old_text.is_empty()) {
        return Err(format!("oldText must not be empty in {file}."));
    }
    let fuzzy_content = normalize_for_fuzzy_match(content);
    let use_fuzzy = edits
        .iter()
        .any(|(old_text, _)| !content.contains(old_text));
    let base = if use_fuzzy {
        fuzzy_content
    } else {
        content.to_string()
    };
    let mut replacements = Vec::new();
    for (index, (old_text, new_text)) in edits.iter().enumerate() {
        let fuzzy_needle = normalize_for_fuzzy_match(old_text);
        let fuzzy_haystack = normalize_for_fuzzy_match(&base);
        let occurrences = fuzzy_haystack.match_indices(&fuzzy_needle).count();
        if occurrences == 0 {
            return Err(format!(
                "Could not find edits[{index}] in {file}. The oldText must match exactly including all whitespace and newlines."
            ));
        }
        if occurrences > 1 {
            return Err(format!(
                "Found {occurrences} occurrences of edits[{index}] in {file}. Each oldText must be unique. Please provide more context to make it unique."
            ));
        }
        let (start, length) = base
            .find(old_text)
            .map(|start| (start, old_text.len()))
            .or_else(|| {
                fuzzy_haystack
                    .find(&fuzzy_needle)
                    .map(|start| (start, fuzzy_needle.len()))
            })
            .unwrap();
        replacements.push((start, start + length, new_text.clone()));
    }
    replacements.sort_by_key(|replacement| replacement.0);
    for pair in replacements.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(format!(
                "edits overlap in {file}. Merge them into one edit or target disjoint regions."
            ));
        }
    }
    Ok((base, replacements))
}

fn normalize_for_fuzzy_match(text: &str) -> String {
    text.nfkc()
        .collect::<String>()
        .split('\n')
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .replace(['\u{2018}', '\u{2019}', '\u{201a}', '\u{201b}'], "'")
        .replace(['\u{201c}', '\u{201d}', '\u{201e}', '\u{201f}'], "\"")
        .replace(
            [
                '\u{2010}', '\u{2011}', '\u{2012}', '\u{2013}', '\u{2014}', '\u{2015}', '\u{2212}',
            ],
            "-",
        )
        .replace(
            [
                '\u{00a0}', '\u{2002}', '\u{2003}', '\u{2004}', '\u{2005}', '\u{2006}', '\u{2007}',
                '\u{2008}', '\u{2009}', '\u{200a}', '\u{202f}', '\u{205f}', '\u{3000}',
            ],
            " ",
        )
}
