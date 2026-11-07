use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Modifier {
    Control,
    Alt,
    Shift,
    Meta,
}

impl Modifier {
    fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => Some(Self::Control),
            "alt" | "option" => Some(Self::Alt),
            "shift" => Some(Self::Shift),
            "meta" | "cmd" | "command" | "super" | "win" | "windows" => Some(Self::Meta),
            _ => None,
        }
    }

    fn canonical_name(self) -> &'static str {
        match self {
            Self::Control => "Ctrl",
            Self::Alt => "Alt",
            Self::Shift => "Shift",
            Self::Meta => "Meta",
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct KeyCode(String);

impl KeyCode {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn parse(value: &str) -> Option<Self> {
        let upper = value.to_ascii_uppercase();
        if upper.len() == 1 && upper.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return Some(Self(upper));
        }

        let canonical = match upper.as_str() {
            "ESC" | "ESCAPE" => "Escape",
            "ENTER" | "RETURN" => "Enter",
            "SPACE" => "Space",
            "TAB" => "Tab",
            "BACKSPACE" | "BACK_SPACE" => "Backspace",
            "DELETE" | "DEL" => "Delete",
            "INSERT" | "INS" => "Insert",
            "HOME" => "Home",
            "END" => "End",
            "PAGEUP" | "PAGE_UP" | "PGUP" => "PageUp",
            "PAGEDOWN" | "PAGE_DOWN" | "PGDN" => "PageDown",
            "LEFT" | "ARROWLEFT" | "ARROW_LEFT" => "ArrowLeft",
            "RIGHT" | "ARROWRIGHT" | "ARROW_RIGHT" => "ArrowRight",
            "UP" | "ARROWUP" | "ARROW_UP" => "ArrowUp",
            "DOWN" | "ARROWDOWN" | "ARROW_DOWN" => "ArrowDown",
            _ => {
                if let Some(number) = upper.strip_prefix('F') {
                    if let Ok(number) = number.parse::<u8>() {
                        if (1..=24).contains(&number) {
                            return Some(Self(format!("F{number}")));
                        }
                    }
                }
                return None;
            }
        };
        Some(Self(canonical.to_owned()))
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct KeyCombo {
    pub modifiers: Vec<Modifier>,
    pub key: KeyCode,
}

impl KeyCombo {
    pub fn parse(input: &str) -> Result<Self, KeyComboParseError> {
        let parts: Vec<_> = input.split('+').map(str::trim).collect();
        if parts.iter().any(|part| part.is_empty()) {
            return Err(KeyComboParseError::EmptyPart);
        }

        let mut modifiers = BTreeSet::new();
        let mut key = None;
        for part in parts {
            if let Some(modifier) = Modifier::parse(part) {
                if !modifiers.insert(modifier) {
                    return Err(KeyComboParseError::DuplicateModifier);
                }
            } else if key.is_some() {
                return Err(KeyComboParseError::MultipleKeys);
            } else if let Some(parsed_key) = KeyCode::parse(part) {
                key = Some(parsed_key);
            } else {
                return Err(KeyComboParseError::UnsupportedKey(part.to_owned()));
            }
        }

        let key = key.ok_or(KeyComboParseError::MissingKey)?;
        Ok(Self {
            modifiers: modifiers.into_iter().collect(),
            key,
        })
    }

    pub fn normalized(&self) -> Result<Self, KeyComboParseError> {
        let mut modifiers = BTreeSet::new();
        for modifier in &self.modifiers {
            if !modifiers.insert(*modifier) {
                return Err(KeyComboParseError::DuplicateModifier);
            }
        }
        let key = KeyCode::parse(self.key.as_str())
            .ok_or_else(|| KeyComboParseError::UnsupportedKey(self.key.as_str().to_owned()))?;
        Ok(Self {
            modifiers: modifiers.into_iter().collect(),
            key,
        })
    }

    pub fn canonical_string(&self) -> Result<String, KeyComboParseError> {
        let normalized = self.normalized()?;
        let mut parts: Vec<_> = normalized
            .modifiers
            .into_iter()
            .map(|modifier| modifier.canonical_name().to_owned())
            .collect();
        parts.push(normalized.key.0);
        Ok(parts.join("+"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum KeyComboParseError {
    EmptyPart,
    MissingKey,
    MultipleKeys,
    DuplicateModifier,
    UnsupportedKey(String),
}

impl std::fmt::Display for KeyComboParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyPart => f.write_str("Key combinations cannot contain empty parts."),
            Self::MissingKey => f.write_str("A key combination must include a key."),
            Self::MultipleKeys => f.write_str("A key combination can include only one key."),
            Self::DuplicateModifier => f.write_str("A modifier is repeated."),
            Self::UnsupportedKey(key) => write!(f, "Unsupported key: {key}."),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HotkeyConfig {
    pub bindings: Vec<HotkeyBinding>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HotkeyBinding {
    pub combo: KeyCombo,
    pub script: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct BindingConflict {
    pub first_index: usize,
    pub duplicate_index: usize,
    pub combo: KeyCombo,
}

pub fn detect_duplicate_bindings(bindings: &[HotkeyBinding]) -> Vec<BindingConflict> {
    let mut seen = Vec::<(KeyCombo, usize)>::new();
    let mut conflicts = Vec::new();
    for (index, binding) in bindings.iter().enumerate() {
        let Ok(combo) = binding.combo.normalized() else {
            continue;
        };
        if let Some((_, first_index)) = seen.iter().find(|(seen_combo, _)| *seen_combo == combo) {
            conflicts.push(BindingConflict {
                first_index: *first_index,
                duplicate_index: index,
                combo,
            });
        } else {
            seen.push((combo, index));
        }
    }
    conflicts
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Panel {
    Watchlist,
    Chart,
    Orders,
    Account,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Timeframe {
    OneMinute,
    FiveMinutes,
    FifteenMinutes,
    OneHour,
    OneDay,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum DryRunAction {
    OpenPanel { panel: Panel },
    SetTimeframe { timeframe: Timeframe },
    ToggleWatchlist,
    Notify { message: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptDiagnostic {
    pub line: usize,
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ScriptLintResult {
    pub actions: Vec<DryRunAction>,
    pub diagnostics: Vec<ScriptDiagnostic>,
}

impl ScriptLintResult {
    pub fn is_valid(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

pub fn lint_script(script: &str) -> ScriptLintResult {
    let mut result = ScriptLintResult::default();
    for (index, raw_line) in script.lines().enumerate() {
        let line_number = index + 1;
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut tokens = line.split_whitespace();
        let command = tokens.next().unwrap_or_default();
        let remainder = line.strip_prefix(command).unwrap_or_default().trim_start();
        let action = match command.to_ascii_lowercase().as_str() {
            "open_panel" => match single_argument(tokens, remainder) {
                Some(value) => match value.to_ascii_lowercase().as_str() {
                    "watchlist" => Some(DryRunAction::OpenPanel {
                        panel: Panel::Watchlist,
                    }),
                    "chart" => Some(DryRunAction::OpenPanel {
                        panel: Panel::Chart,
                    }),
                    "orders" => Some(DryRunAction::OpenPanel {
                        panel: Panel::Orders,
                    }),
                    "account" => Some(DryRunAction::OpenPanel {
                        panel: Panel::Account,
                    }),
                    _ => {
                        result.diagnostics.push(diagnostic(
                            line_number,
                            "invalid_panel",
                            "open_panel expects watchlist, chart, orders, or account.",
                        ));
                        None
                    }
                },
                None => {
                    result.diagnostics.push(diagnostic(
                        line_number,
                        "invalid_arguments",
                        "open_panel requires exactly one panel name.",
                    ));
                    None
                }
            },
            "set_timeframe" => match single_argument(tokens, remainder) {
                Some(value) => match value.to_ascii_lowercase().as_str() {
                    "1m" => Some(DryRunAction::SetTimeframe {
                        timeframe: Timeframe::OneMinute,
                    }),
                    "5m" => Some(DryRunAction::SetTimeframe {
                        timeframe: Timeframe::FiveMinutes,
                    }),
                    "15m" => Some(DryRunAction::SetTimeframe {
                        timeframe: Timeframe::FifteenMinutes,
                    }),
                    "1h" => Some(DryRunAction::SetTimeframe {
                        timeframe: Timeframe::OneHour,
                    }),
                    "1d" => Some(DryRunAction::SetTimeframe {
                        timeframe: Timeframe::OneDay,
                    }),
                    _ => {
                        result.diagnostics.push(diagnostic(
                            line_number,
                            "invalid_timeframe",
                            "set_timeframe expects 1m, 5m, 15m, 1h, or 1d.",
                        ));
                        None
                    }
                },
                None => {
                    result.diagnostics.push(diagnostic(
                        line_number,
                        "invalid_arguments",
                        "set_timeframe requires exactly one timeframe.",
                    ));
                    None
                }
            },
            "toggle_watchlist" => {
                if remainder.is_empty() {
                    Some(DryRunAction::ToggleWatchlist)
                } else {
                    result.diagnostics.push(diagnostic(
                        line_number,
                        "invalid_arguments",
                        "toggle_watchlist takes no arguments.",
                    ));
                    None
                }
            }
            "notify" => {
                if remainder.is_empty() {
                    result.diagnostics.push(diagnostic(
                        line_number,
                        "invalid_arguments",
                        "notify requires a non-empty message.",
                    ));
                    None
                } else if remainder.chars().any(char::is_control) {
                    result.diagnostics.push(diagnostic(
                        line_number,
                        "invalid_message",
                        "notify messages cannot contain control characters.",
                    ));
                    None
                } else {
                    Some(DryRunAction::Notify {
                        message: remainder.to_owned(),
                    })
                }
            }
            _ => {
                result.diagnostics.push(diagnostic(
                    line_number,
                    "unsupported_command",
                    "Unsupported script command; only local UI actions are allowed.",
                ));
                None
            }
        };
        if let Some(action) = action {
            result.actions.push(action);
        }
    }
    result
}

pub fn dry_run_script(script: &str) -> Result<Vec<DryRunAction>, Vec<ScriptDiagnostic>> {
    let result = lint_script(script);
    if result.is_valid() {
        Ok(result.actions)
    } else {
        Err(result.diagnostics)
    }
}

fn single_argument<'a>(
    mut tokens: impl Iterator<Item = &'a str>,
    remainder: &'a str,
) -> Option<&'a str> {
    let argument = tokens.next()?;
    if tokens.next().is_some() || argument != remainder {
        return None;
    }
    Some(argument)
}

fn diagnostic(line: usize, code: &str, message: &str) -> ScriptDiagnostic {
    ScriptDiagnostic {
        line,
        severity: DiagnosticSeverity::Error,
        code: code.to_owned(),
        message: message.to_owned(),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HotkeyEdit {
    pub bindings: Vec<StagedHotkeyBinding>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct StagedHotkeyBinding {
    pub combo: String,
    pub script: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum HotkeyValidationErrorKind {
    InvalidCombo { message: String },
    DuplicateCombo { first_index: usize, combo: String },
    InvalidScript { diagnostics: Vec<ScriptDiagnostic> },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct HotkeyValidationError {
    pub binding_index: usize,
    pub kind: HotkeyValidationErrorKind,
}

impl HotkeyEdit {
    pub fn validate(&self) -> Vec<HotkeyValidationError> {
        let mut errors = Vec::new();
        let mut seen = Vec::<(KeyCombo, usize)>::new();
        for (binding_index, binding) in self.bindings.iter().enumerate() {
            match KeyCombo::parse(&binding.combo) {
                Ok(combo) => {
                    if let Some((_, first_index)) =
                        seen.iter().find(|(seen_combo, _)| *seen_combo == combo)
                    {
                        errors.push(HotkeyValidationError {
                            binding_index,
                            kind: HotkeyValidationErrorKind::DuplicateCombo {
                                first_index: *first_index,
                                combo: combo
                                    .canonical_string()
                                    .expect("parsed combos are canonicalizable"),
                            },
                        });
                    } else {
                        seen.push((combo, binding_index));
                    }
                }
                Err(error) => errors.push(HotkeyValidationError {
                    binding_index,
                    kind: HotkeyValidationErrorKind::InvalidCombo {
                        message: error.to_string(),
                    },
                }),
            }

            let lint = lint_script(&binding.script);
            if !lint.is_valid() {
                errors.push(HotkeyValidationError {
                    binding_index,
                    kind: HotkeyValidationErrorKind::InvalidScript {
                        diagnostics: lint.diagnostics,
                    },
                });
            }
        }
        errors
    }

    pub fn commit(&self) -> Result<HotkeyConfig, Vec<HotkeyValidationError>> {
        let errors = self.validate();
        if !errors.is_empty() {
            return Err(errors);
        }
        let bindings = self
            .bindings
            .iter()
            .map(|binding| HotkeyBinding {
                combo: KeyCombo::parse(&binding.combo)
                    .expect("validated combo must parse")
                    .normalized()
                    .expect("validated combo must normalize"),
                script: binding.script.clone(),
            })
            .collect();
        Ok(HotkeyConfig { bindings })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(combo: &str, script: &str) -> HotkeyBinding {
        HotkeyBinding {
            combo: KeyCombo::parse(combo).unwrap(),
            script: script.to_owned(),
        }
    }

    #[test]
    fn parses_modifier_aliases_into_stable_canonical_combinations() {
        for (input, expected) in [
            ("control + a", "Ctrl+A"),
            ("CTRL+shift+f2", "Ctrl+Shift+F2"),
            ("option+ESC", "Alt+Escape"),
            ("cmd+arrow_left", "Meta+ArrowLeft"),
            ("windows+pgdn", "Meta+PageDown"),
        ] {
            assert_eq!(
                KeyCombo::parse(input).unwrap().canonical_string().unwrap(),
                expected
            );
        }
    }

    #[test]
    fn rejects_malformed_combinations_without_silently_discarding_parts() {
        for input in [
            "",
            "+A",
            "Ctrl+",
            "Ctrl++A",
            "Ctrl+Control+A",
            "A+B",
            "Ctrl+💥",
        ] {
            assert!(KeyCombo::parse(input).is_err(), "accepted {input:?}");
        }
    }

    #[test]
    fn detects_duplicate_bindings_after_alias_normalization() {
        let edit = HotkeyEdit {
            bindings: vec![
                StagedHotkeyBinding {
                    combo: "control+a".into(),
                    script: "toggle_watchlist".into(),
                },
                StagedHotkeyBinding {
                    combo: "Ctrl+A".into(),
                    script: "open_panel chart".into(),
                },
            ],
        };
        let errors = edit.validate();
        assert!(matches!(
            &errors[0].kind,
            HotkeyValidationErrorKind::DuplicateCombo { first_index: 0, combo }
                if combo == "Ctrl+A"
        ));
        assert!(edit.commit().is_err());
    }

    #[test]
    fn lint_collects_errors_for_malformed_and_unsupported_script_lines() {
        let lint = lint_script(
            "open_panel\n\
             set_timeframe 2m\n\
             place_order buy SPY 100\n\
             notify \n\
             toggle_watchlist unexpected",
        );
        assert_eq!(lint.diagnostics.len(), 5);
        assert_eq!(lint.diagnostics[0].line, 1);
        assert_eq!(lint.diagnostics[2].code, "unsupported_command");
        assert!(!lint.is_valid());
        assert!(dry_run_script("toggle_watchlist\nplace_order buy SPY").is_err());
    }

    #[test]
    fn safe_scripts_produce_only_data_actions_for_dry_run() {
        let actions = dry_run_script(
            "# comments are inert\nopen_panel chart\nset_timeframe 5m\ntoggle_watchlist\nnotify Ready",
        )
        .unwrap();
        assert_eq!(
            actions,
            vec![
                DryRunAction::OpenPanel {
                    panel: Panel::Chart
                },
                DryRunAction::SetTimeframe {
                    timeframe: Timeframe::FiveMinutes,
                },
                DryRunAction::ToggleWatchlist,
                DryRunAction::Notify {
                    message: "Ready".into(),
                },
            ]
        );
    }

    #[test]
    fn duplicate_detection_ignores_invalid_combo_entries() {
        let bindings = vec![
            binding("Ctrl+A", "toggle_watchlist"),
            HotkeyBinding {
                combo: KeyCombo {
                    modifiers: vec![],
                    key: KeyCode("script".into()),
                },
                script: "toggle_watchlist".into(),
            },
            binding("Control+a", "open_panel chart"),
        ];
        let conflicts = detect_duplicate_bindings(&bindings);
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].first_index, 0);
        assert_eq!(conflicts[0].duplicate_index, 2);
    }
}
