use super::App;
use crossterm::event::{KeyCode, KeyModifiers};
use jcode_tui_messages::DisplayMessage;
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShortcutAction {
    ToggleAutoPoke,
    ToggleQueueMode,
    OpenHistorySearch,
    OpenSessionPicker,
    ClearView,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum CommandPaletteAction {
    SlashCommand(String),
    Shortcut(ShortcutAction),
    ProfileSelect(Option<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CommandPaletteEntry {
    pub kind: &'static str,
    pub command: String,
    pub description: String,
    pub action: CommandPaletteAction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CommandPaletteState {
    pub query: String,
    pub selected: usize,
    pub mode: CommandPaletteMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CommandPaletteMode {
    Commands,
    Profiles,
}

impl App {
    fn profile_startup_metadata(
        name: &str,
    ) -> anyhow::Result<crate::protocol::SessionProfileStartup> {
        let config = crate::config::config();
        let resolved = config.resolve_session_profile(Some(name))?;
        let selection = resolved.tool_selection(&config.tools);
        let mut allowed_tools = selection.allowed_tools.map(|tools| {
            let mut tools = tools.into_iter().collect::<Vec<_>>();
            tools.sort_unstable();
            tools
        });
        let mut disabled_tools = selection.disabled_tools.into_iter().collect::<Vec<_>>();
        disabled_tools.sort_unstable();
        Ok(crate::protocol::SessionProfileStartup {
            profile_name: Some(name.to_owned()),
            provider: resolved.provider,
            model: resolved.model,
            provider_profile: resolved.provider_profile,
            reasoning_effort: resolved.reasoning_effort,
            allowed_tools: allowed_tools.take(),
            disabled_tools,
            skill_names: resolved.skill_policy.selected_skills,
            skills_mode: resolved.skill_policy.mode,
            disabled_skills: resolved.skill_policy.disabled_skills,
            skill_prompts: resolved.prompt_overlay.skill_prompts,
            instructions: resolved.prompt_overlay.instructions,
        })
    }

    pub(super) fn request_profile_transition(&mut self, profile: Option<String>) -> bool {
        let startup = match profile.as_deref() {
            Some(name) => match Self::profile_startup_metadata(name) {
                Ok(startup) => Some(startup),
                Err(error) => {
                    self.push_display_message(DisplayMessage::error(format!(
                        "Profile selection failed: {error}"
                    )));
                    self.set_status_notice("Profile selection failed");
                    return false;
                }
            },
            None => None,
        };

        if let Some(name) = profile.as_deref()
            && !self
                .profile_state
                .configured_names
                .iter()
                .any(|configured| configured == name)
        {
            self.push_display_message(DisplayMessage::error(format!(
                "Unknown session profile '{}'; available profiles: {}",
                name,
                if self.profile_state.configured_names.is_empty() {
                    "(none)".to_string()
                } else {
                    self.profile_state.configured_names.join(", ")
                }
            )));
            self.set_status_notice("Unknown session profile");
            return false;
        }

        self.profile_state.pending = Some(profile.clone());
        self.profile_state.pending_startup = Some(startup);
        self.set_status_notice(match profile.as_deref() {
            Some(name) => format!("Profile '{}' queued for next turn", name),
            None => "No profile queued for next turn".to_string(),
        });
        true
    }

    pub(super) fn commit_pending_profile_transition(&mut self) -> bool {
        let Some(profile) = self.profile_state.pending.take() else {
            return false;
        };
        let startup = self.profile_state.pending_startup.take().flatten();
        self.profile_state.current = profile.clone();
        self.profile_state.current_startup = startup;
        self.context_revision = self.context_revision.wrapping_add(1);
        self.invalidate_command_candidates_cache();
        self.set_status_notice(match profile.as_deref() {
            Some(name) => format!("Profile '{}' active", name),
            None => "No profile active".to_string(),
        });
        true
    }

    #[cfg(test)]
    pub(super) fn set_profile_names_for_test<I, S>(&mut self, names: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.profile_state.configured_names = names
            .into_iter()
            .map(|name| name.as_ref().to_owned())
            .collect();
        self.profile_state.configured_names.sort_unstable();
        self.invalidate_command_candidates_cache();
    }

    #[cfg(test)]
    pub(super) fn set_current_profile_for_test(&mut self, profile: Option<&str>) {
        self.profile_state.current = profile.map(str::to_owned);
        self.profile_state.current_startup =
            profile.and_then(|name| Self::profile_startup_metadata(name).ok());
    }

    #[cfg(test)]
    pub(super) fn current_profile_for_test(&self) -> Option<String> {
        self.profile_state.current.clone()
    }

    #[cfg(test)]
    pub(super) fn pending_profile_for_test(&self) -> Option<Option<String>> {
        self.profile_state.pending.clone()
    }

    pub(super) fn open_command_palette(&mut self) -> bool {
        self.command_palette = Some(CommandPaletteState {
            query: String::new(),
            selected: 0,
            mode: CommandPaletteMode::Commands,
        });
        true
    }

    pub(super) fn open_profile_picker(&mut self) -> bool {
        self.command_palette = Some(CommandPaletteState {
            query: String::new(),
            selected: 0,
            mode: CommandPaletteMode::Profiles,
        });
        true
    }

    fn close_command_palette(&mut self) {
        self.command_palette = None;
    }

    fn command_palette_entries(&self) -> Vec<CommandPaletteEntry> {
        if self
            .command_palette
            .as_ref()
            .is_some_and(|state| state.mode == CommandPaletteMode::Profiles)
        {
            return self.profile_picker_entries();
        }

        let skills = self.current_skills_snapshot();
        let mut skill_names: HashSet<String> = skills
            .list()
            .into_iter()
            .map(|skill| format!("/{}", skill.name))
            .collect();
        skill_names.extend(
            self.remote_skills
                .iter()
                .filter(|name| self.profile_allows_skill_name(name))
                .map(|name| format!("/{name}")),
        );

        let mut entries = self
            .command_candidates()
            .into_iter()
            .map(|(command, description)| {
                let kind = if skill_names.contains(&command) {
                    "Skill"
                } else {
                    "Command"
                };
                CommandPaletteEntry {
                    kind,
                    action: CommandPaletteAction::SlashCommand(command.clone()),
                    command,
                    description: description.to_string(),
                }
            })
            .collect::<Vec<_>>();

        entries.extend([
            Self::shortcut_entry(
                "Toggle auto-poke",
                "Enable or disable automatic todo follow-ups",
                ShortcutAction::ToggleAutoPoke,
            ),
            Self::shortcut_entry(
                "Toggle queue mode",
                "Choose whether prompts wait for the current turn",
                ShortcutAction::ToggleQueueMode,
            ),
            Self::shortcut_entry(
                "Open prompt history",
                "Search prompts across sessions",
                ShortcutAction::OpenHistorySearch,
            ),
            Self::shortcut_entry(
                "Open session picker",
                "Resume or switch to another session",
                ShortcutAction::OpenSessionPicker,
            ),
            Self::shortcut_entry(
                "Clear terminal view",
                "Push the transcript into scrollback without deleting it",
                ShortcutAction::ClearView,
            ),
        ]);
        entries
    }

    fn profile_picker_entries(&self) -> Vec<CommandPaletteEntry> {
        let mut entries = self
            .profile_state
            .configured_names
            .iter()
            .cloned()
            .map(|name| {
                let active = self.profile_state.current.as_deref() == Some(name.as_str());
                CommandPaletteEntry {
                    kind: "Profile",
                    command: name.clone(),
                    description: if active {
                        "Select profile (active)".to_string()
                    } else {
                        "Select profile".to_string()
                    },
                    action: CommandPaletteAction::ProfileSelect(Some(name)),
                }
            })
            .collect::<Vec<_>>();

        let no_profiles = entries.is_empty();
        entries.push(CommandPaletteEntry {
            kind: "Profile",
            command: "none".to_string(),
            description: if no_profiles {
                "No configured profiles; use none for an unprofiled session".to_string()
            } else if self.profile_state.current.is_none() {
                "Clear profile (active)".to_string()
            } else {
                "Clear profile".to_string()
            },
            action: CommandPaletteAction::ProfileSelect(None),
        });
        entries
    }

    fn shortcut_entry(
        command: &'static str,
        description: &'static str,
        action: ShortcutAction,
    ) -> CommandPaletteEntry {
        CommandPaletteEntry {
            kind: "Shortcut",
            command: command.to_string(),
            description: description.to_string(),
            action: CommandPaletteAction::Shortcut(action),
        }
    }

    fn filtered_command_palette_entries(&self) -> Vec<CommandPaletteEntry> {
        let Some(state) = self.command_palette.as_ref() else {
            return Vec::new();
        };
        let query = state.query.trim().to_ascii_lowercase();
        let mut entries = self.command_palette_entries();
        if query.is_empty() {
            return entries;
        }

        entries.retain(|entry| {
            let haystack = format!("{} {} {}", entry.kind, entry.command, entry.description)
                .to_ascii_lowercase();
            haystack.contains(&query) || crate::tui::fuzzy::fuzzy_score(&query, &haystack).is_some()
        });
        entries
    }

    pub(crate) fn command_palette_view(&self) -> Option<crate::tui::CommandPaletteView> {
        let state = self.command_palette.as_ref()?;
        let entries = self.filtered_command_palette_entries();
        Some(crate::tui::CommandPaletteView {
            query: state.query.clone(),
            selected: state.selected.min(entries.len().saturating_sub(1)),
            entries: entries
                .into_iter()
                .map(|entry| crate::tui::CommandPaletteEntryView {
                    kind: entry.kind,
                    command: entry.command,
                    description: entry.description,
                })
                .collect(),
        })
    }

    fn move_command_palette_selection(&mut self, delta: i32) {
        let len = self.filtered_command_palette_entries().len();
        if len == 0 {
            if let Some(state) = self.command_palette.as_mut() {
                state.selected = 0;
            }
            return;
        }
        let selected = self
            .command_palette
            .as_ref()
            .map(|state| state.selected.min(len - 1))
            .unwrap_or(0) as i32;
        if let Some(state) = self.command_palette.as_mut() {
            state.selected = (selected + delta).rem_euclid(len as i32) as usize;
        }
    }

    pub(super) fn handle_command_palette_key(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) -> bool {
        if self.command_palette.is_none() {
            return false;
        }

        let plain = !modifiers.intersects(
            KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SUPER | KeyModifiers::HYPER,
        );
        match code {
            KeyCode::Esc => self.close_command_palette(),
            KeyCode::Up if plain => self.move_command_palette_selection(-1),
            KeyCode::Down if plain => self.move_command_palette_selection(1),
            KeyCode::Char('k') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_command_palette_selection(-1)
            }
            KeyCode::Char('j') | KeyCode::Char('n')
                if modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.move_command_palette_selection(1)
            }
            KeyCode::Char('p') if modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_command_palette_selection(1)
            }
            KeyCode::Backspace if plain => {
                if let Some(state) = self.command_palette.as_mut() {
                    state.query.pop();
                    state.selected = 0;
                }
            }
            KeyCode::Char(c) if plain => {
                if let Some(state) = self.command_palette.as_mut() {
                    state.query.push(c);
                    state.selected = 0;
                }
            }
            KeyCode::Enter if modifiers.is_empty() => {
                if let Some(action) = self.accept_command_palette_selection() {
                    self.dispatch_command_palette_action(action);
                }
            }
            _ => {}
        }
        true
    }

    pub(super) fn accept_command_palette_selection(&mut self) -> Option<CommandPaletteAction> {
        let entries = self.filtered_command_palette_entries();
        let Some(state) = self.command_palette.as_ref() else {
            return None;
        };
        let Some(entry) = entries.get(state.selected.min(entries.len().saturating_sub(1))) else {
            self.set_status_notice("No matching commands");
            return None;
        };
        let action = entry.action.clone();
        self.close_command_palette();
        Some(action)
    }

    pub(super) fn dispatch_command_palette_action(&mut self, action: CommandPaletteAction) {
        match action {
            CommandPaletteAction::SlashCommand(command) => {
                // `/model` and `/models` normally open a preview while typed in the
                // composer. From the command palette, the command is already an
                // explicit selection, so open the full picker instead of simulating
                // typing the command and immediately activating its first row.
                if matches!(command.as_str(), "/model" | "/models") {
                    self.open_model_picker();
                    return;
                }
                self.input = command;
                self.cursor_pos = self.input.len();
                self.sync_model_picker_preview_from_input();
                super::input::handle_enter(self);
            }
            CommandPaletteAction::ProfileSelect(profile) => {
                self.request_profile_transition(profile);
            }
            CommandPaletteAction::Shortcut(action) => match action {
                ShortcutAction::ToggleAutoPoke => {
                    super::commands::toggle_auto_poke_hotkey_local(self)
                }
                ShortcutAction::ToggleQueueMode => {
                    self.queue_mode = !self.queue_mode;
                    self.set_status_notice(if self.queue_mode {
                        "Queue mode: messages wait until response completes"
                    } else {
                        "Immediate mode: messages send next (no interrupt)"
                    });
                }
                ShortcutAction::OpenHistorySearch => self.open_prompt_history_search(),
                ShortcutAction::OpenSessionPicker => self.open_session_picker(),
                ShortcutAction::ClearView => self.clear_view_terminal_style(),
            },
        }
    }

    #[cfg(test)]
    pub(super) fn command_palette_is_open(&self) -> bool {
        self.command_palette.is_some()
    }

    #[cfg(test)]
    pub(super) fn profile_picker_is_open(&self) -> bool {
        self.command_palette
            .as_ref()
            .is_some_and(|state| state.mode == CommandPaletteMode::Profiles)
    }

    #[cfg(test)]
    pub(super) fn command_palette_query(&self) -> Option<&str> {
        self.command_palette
            .as_ref()
            .map(|state| state.query.as_str())
    }

    #[cfg(test)]
    pub(super) fn command_palette_entry_kinds(&self) -> Vec<&'static str> {
        self.command_palette_entries()
            .into_iter()
            .map(|entry| entry.kind)
            .collect()
    }

    #[cfg(test)]
    pub(super) fn command_palette_selected(&self) -> usize {
        self.command_palette
            .as_ref()
            .map(|s| s.selected)
            .unwrap_or(0)
    }
}
